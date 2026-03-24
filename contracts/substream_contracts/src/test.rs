#![cfg(test)]

use super::*;
use soroban_sdk::{
    testutils::{Address as _, Ledger},
    token, Address, Env,
};

fn create_token_contract<'a>(env: &Env, admin: &Address) -> token::Client<'a> {
    let sac = env.register_stellar_asset_contract_v2(admin.clone());
    token::Client::new(env, &sac.address())
}

#[test]
fn test_subscribe_and_collect() {
    let env = Env::default();
    env.mock_all_auths();

    let subscriber = Address::generate(&env);
    let creator = Address::generate(&env);
    let admin = Address::generate(&env);

    let token = create_token_contract(&env, &admin);
    let token_admin = token::StellarAssetClient::new(&env, &token.address);
    token_admin.mint(&subscriber, &1000);

    let contract_id = env.register(SubStreamContract, ());
    let client = SubStreamContractClient::new(&env, &contract_id);

    // Initial timestamp
    env.ledger().set_timestamp(100);

    // Subscribe: 100 tokens, rate 2 per second
    client.subscribe(&subscriber, &creator, &token.address, &100, &2);

    assert_eq!(token.balance(&subscriber), 900);
    assert_eq!(token.balance(&contract_id), 100);

    // Advance 10 seconds
    env.ledger().set_timestamp(110);

    // Collect: 10 secs * 2 tokens/sec = 20 tokens
    client.collect(&subscriber, &creator);

    assert_eq!(token.balance(&contract_id), 80);
    assert_eq!(token.balance(&creator), 20);

    // Advance 50 seconds (would be 100 tokens, but only 80 left in balance)
    env.ledger().set_timestamp(160);
    client.collect(&subscriber, &creator);

    assert_eq!(token.balance(&contract_id), 0);
    assert_eq!(token.balance(&creator), 100);
}

#[test]
fn test_cancel() {
    let env = Env::default();
    env.mock_all_auths();

    let subscriber = Address::generate(&env);
    let creator = Address::generate(&env);
    let admin = Address::generate(&env);

    let token = create_token_contract(&env, &admin);
    let token_admin = token::StellarAssetClient::new(&env, &token.address);
    token_admin.mint(&subscriber, &1000);

    let contract_id = env.register(SubStreamContract, ());
    let client = SubStreamContractClient::new(&env, &contract_id);

    env.ledger().set_timestamp(100);

    // Subscribe: 100 tokens, 1 token/sec
    client.subscribe(&subscriber, &creator, &token.address, &100, &1);

    env.ledger().set_timestamp(120); // 20 seconds pass

    // Cancel should collect 20 for creator, refund 80 to subscriber
    client.cancel(&subscriber, &creator);

    assert_eq!(token.balance(&creator), 20);
    assert_eq!(token.balance(&subscriber), 980);
    assert_eq!(token.balance(&contract_id), 0);
}

#[test]
#[should_panic(expected = "amount and rate must be positive")]
fn test_subscribe_invalid_amounts() {
    let env = Env::default();
    env.mock_all_auths();

    let subscriber = Address::generate(&env);
    let creator = Address::generate(&env);
    let token = Address::generate(&env);

    let contract_id = env.register(SubStreamContract, ());
    let client = SubStreamContractClient::new(&env, &contract_id);

    client.subscribe(&subscriber, &creator, &token, &0, &2);
}

#[test]
#[should_panic(expected = "stream already exists")]
fn test_subscribe_already_exists() {
    let env = Env::default();
    env.mock_all_auths();

    let subscriber = Address::generate(&env);
    let creator = Address::generate(&env);
    let admin = Address::generate(&env);

    let token = create_token_contract(&env, &admin);
    let token_admin = token::StellarAssetClient::new(&env, &token.address);
    token_admin.mint(&subscriber, &1000);

    let contract_id = env.register(SubStreamContract, ());
    let client = SubStreamContractClient::new(&env, &contract_id);

    client.subscribe(&subscriber, &creator, &token.address, &100, &2);
    // Should panic here
    client.subscribe(&subscriber, &creator, &token.address, &100, &2);
}

#[test]
fn test_top_up() {
    let env = Env::default();
    env.mock_all_auths();

    let subscriber = Address::generate(&env);
    let creator = Address::generate(&env);
    let admin = Address::generate(&env);

    let token = create_token_contract(&env, &admin);
    let token_admin = token::StellarAssetClient::new(&env, &token.address);
    token_admin.mint(&subscriber, &1000);

    let contract_id = env.register(SubStreamContract, ());
    let client = SubStreamContractClient::new(&env, &contract_id);

    // Initial subscribe
    client.subscribe(&subscriber, &creator, &token.address, &100, &1);
    assert_eq!(token.balance(&contract_id), 100);

    // Top up
    client.top_up(&subscriber, &creator, &50);
    assert_eq!(token.balance(&contract_id), 150);

    // Verify it still works with the new balance
    env.ledger().set_timestamp(120); // 120 seconds pass
    client.collect(&subscriber, &creator);
    assert_eq!(token.balance(&creator), 120);
    assert_eq!(token.balance(&contract_id), 30);
}

#[test]
fn test_cliff_based_access_no_threshold() {
    let env = Env::default();
    env.mock_all_auths();

    let subscriber = Address::generate(&env);
    let creator = Address::generate(&env);
    let admin = Address::generate(&env);

    let token = create_token_contract(&env, &admin);
    let token_admin = token::StellarAssetClient::new(&env, &token.address);
    token_admin.mint(&subscriber, &1000);

    let contract_id = env.register(SubStreamContract, ());
    let client = SubStreamContractClient::new(&env, &contract_id);

    assert!(client.has_unlocked_access(&subscriber, &creator));
    assert_eq!(client.get_access_tier(&subscriber, &creator), 2);
}

#[test]
fn test_cliff_based_access_before_threshold() {
    let env = Env::default();
    env.mock_all_auths();

    let subscriber = Address::generate(&env);
    let creator = Address::generate(&env);
    let admin = Address::generate(&env);

    let token = create_token_contract(&env, &admin);
    let token_admin = token::StellarAssetClient::new(&env, &token.address);
    token_admin.mint(&subscriber, &1000);

    let contract_id = env.register(SubStreamContract, ());
    let client = SubStreamContractClient::new(&env, &contract_id);

    client.set_cliff_threshold(&creator, &50);
    assert_eq!(client.get_cliff_threshold(&creator), 50);

    assert!(!client.has_unlocked_access(&subscriber, &creator));
    assert_eq!(client.get_access_tier(&subscriber, &creator), 0);

    client.subscribe(&subscriber, &creator, &token.address, &30, &1);
    env.ledger().set_timestamp(100);
    client.collect(&subscriber, &creator);

    assert!(!client.has_unlocked_access(&subscriber, &creator));
    assert_eq!(client.get_access_tier(&subscriber, &creator), 0);
}

#[test]
fn test_cliff_based_access_after_threshold() {
    let env = Env::default();
    env.mock_all_auths();

    let subscriber = Address::generate(&env);
    let creator = Address::generate(&env);
    let admin = Address::generate(&env);

    let token = create_token_contract(&env, &admin);
    let token_admin = token::StellarAssetClient::new(&env, &token.address);
    token_admin.mint(&subscriber, &1000);

    let contract_id = env.register(SubStreamContract, ());
    let client = SubStreamContractClient::new(&env, &contract_id);

    client.set_cliff_threshold(&creator, &50);

    client.subscribe(&subscriber, &creator, &token.address, &100, &1);

    assert!(!client.has_unlocked_access(&subscriber, &creator));

    env.ledger().set_timestamp(100);
    client.collect(&subscriber, &creator);

    assert!(client.has_unlocked_access(&subscriber, &creator));
    assert_eq!(client.get_total_streamed(&subscriber, &creator), 100);
    assert_eq!(client.get_access_tier(&subscriber, &creator), 1);
}

#[test]
fn test_access_tiers() {
    let env = Env::default();
    env.mock_all_auths();

    let subscriber = Address::generate(&env);
    let creator = Address::generate(&env);
    let admin = Address::generate(&env);

    let token = create_token_contract(&env, &admin);
    let token_admin = token::StellarAssetClient::new(&env, &token.address);
    token_admin.mint(&subscriber, &10000);

    let contract_id = env.register(SubStreamContract, ());
    let client = SubStreamContractClient::new(&env, &contract_id);

    client.set_cliff_threshold(&creator, &50);

    client.subscribe(&subscriber, &creator, &token.address, &60, &2);

    env.ledger().set_timestamp(100);
    client.collect(&subscriber, &creator);

    assert_eq!(client.get_access_tier(&subscriber, &creator), 1);
    assert!(client.has_unlocked_access(&subscriber, &creator));

    client.top_up(&subscriber, &creator, &200);
    env.ledger().set_timestamp(200);
    client.collect(&subscriber, &creator);

    assert_eq!(client.get_access_tier(&subscriber, &creator), 2);
    assert!(client.has_unlocked_access(&subscriber, &creator));

    client.top_up(&subscriber, &creator, &300);
    env.ledger().set_timestamp(400);
    client.collect(&subscriber, &creator);

    assert_eq!(client.get_access_tier(&subscriber, &creator), 3);
    assert!(client.has_unlocked_access(&subscriber, &creator));
}

#[test]
fn test_total_streamed_tracking() {
    let env = Env::default();
    env.mock_all_auths();

    let subscriber = Address::generate(&env);
    let creator = Address::generate(&env);
    let admin = Address::generate(&env);

    let token = create_token_contract(&env, &admin);
    let token_admin = token::StellarAssetClient::new(&env, &token.address);
    token_admin.mint(&subscriber, &1000);

    let contract_id = env.register(SubStreamContract, ());
    let client = SubStreamContractClient::new(&env, &contract_id);

    client.subscribe(&subscriber, &creator, &token.address, &100, &1);

    env.ledger().set_timestamp(100);
    client.collect(&subscriber, &creator);
    assert_eq!(client.get_total_streamed(&subscriber, &creator), 100);

    client.top_up(&subscriber, &creator, &50);
    env.ledger().set_timestamp(150);
    client.collect(&subscriber, &creator);
    assert_eq!(client.get_total_streamed(&subscriber, &creator), 150);
}
