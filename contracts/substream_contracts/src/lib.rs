#![no_std]
use soroban_sdk::{contract, contractimpl, contracttype, vec, Address, Env, Vec};
use soroban_sdk::token::Client as TokenClient;

#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum DataKey {
    Stream(Address, Address),      // (subscriber, creator)
    CreatorSubscribers(Address),   // creator -> Vec<subscriber>
}

#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Stream {
    pub token: Address,
    pub rate_per_second: i128,
    pub balance: i128,
    pub last_collected: u64,
}

#[contract]
pub struct SubStreamContract;

#[contractimpl]
impl SubStreamContract {
    pub fn subscribe(
        env: Env,
        subscriber: Address,
        creator: Address,
        token: Address,
        amount: i128,
        rate_per_second: i128,
    ) {
        subscriber.require_auth();
        
        if amount <= 0 || rate_per_second <= 0 {
            panic!("amount and rate must be positive");
        }

        let key = DataKey::Stream(subscriber.clone(), creator.clone());
        if env.storage().persistent().has(&key) {
            panic!("stream already exists");
        }

        let token_client = TokenClient::new(&env, &token);
        token_client.transfer(&subscriber, &env.current_contract_address(), &amount);

        let stream = Stream {
            token,
            rate_per_second,
            balance: amount,
            last_collected: env.ledger().timestamp(),
        };

        env.storage().persistent().set(&key, &stream);

        // Track subscriber under this creator for withdraw_all
        let creator_key = DataKey::CreatorSubscribers(creator.clone());
        let mut subs: Vec<Address> = env.storage().persistent()
            .get(&creator_key)
            .unwrap_or(vec![&env]);
        subs.push_back(subscriber);
        env.storage().persistent().set(&creator_key, &subs);
    }

    pub fn collect(env: Env, subscriber: Address, creator: Address) {
        let key = DataKey::Stream(subscriber.clone(), creator.clone());
        if !env.storage().persistent().has(&key) {
            panic!("stream not found");
        }

        let mut stream: Stream = env.storage().persistent().get(&key).unwrap();
        let current_time = env.ledger().timestamp();
        
        if current_time <= stream.last_collected {
            return;
        }

        let time_elapsed = (current_time - stream.last_collected) as i128;
        let mut amount_to_collect = time_elapsed * stream.rate_per_second;

        if amount_to_collect > stream.balance {
            amount_to_collect = stream.balance;
        }

        if amount_to_collect > 0 {
            let token_client = TokenClient::new(&env, &stream.token);
            token_client.transfer(&env.current_contract_address(), &creator, &amount_to_collect);
            
            stream.balance -= amount_to_collect;
            stream.last_collected = current_time;
            
            env.storage().persistent().set(&key, &stream);
        }
    }

    pub fn cancel(env: Env, subscriber: Address, creator: Address) {
        subscriber.require_auth();

        let key = DataKey::Stream(subscriber.clone(), creator.clone());
        if !env.storage().persistent().has(&key) {
            panic!("stream not found");
        }

        // First collect any pending amount
        Self::collect(env.clone(), subscriber.clone(), creator.clone());

        // Get updated stream
        let stream: Stream = env.storage().persistent().get(&key).unwrap();
        
        // Refund remaining balance to subscriber
        if stream.balance > 0 {
            let token_client = TokenClient::new(&env, &stream.token);
            token_client.transfer(&env.current_contract_address(), &subscriber, &stream.balance);
        }

        // Remove the stream from storage
        env.storage().persistent().remove(&key);

        // Remove subscriber from creator's subscriber list
        let creator_key = DataKey::CreatorSubscribers(creator.clone());
        if let Some(subs) = env.storage().persistent().get::<DataKey, Vec<Address>>(&creator_key) {
            let mut updated: Vec<Address> = vec![&env];
            for s in subs.iter() {
                if s != subscriber {
                    updated.push_back(s);
                }
            }
            env.storage().persistent().set(&creator_key, &updated);
        }
    }

    pub fn top_up(env: Env, subscriber: Address, creator: Address, amount: i128) {
        subscriber.require_auth();
        if amount <= 0 {
            panic!("amount must be positive");
        }

        let key = DataKey::Stream(subscriber.clone(), creator.clone());
        if !env.storage().persistent().has(&key) {
            panic!("stream not found");
        }

        let mut stream: Stream = env.storage().persistent().get(&key).unwrap();
        let token_client = TokenClient::new(&env, &stream.token);
        token_client.transfer(&subscriber, &env.current_contract_address(), &amount);

        stream.balance += amount;
        env.storage().persistent().set(&key, &stream);
    }
    /// Collect from all active streams for a creator in a single call.
    /// `max_count` caps the batch size to avoid hitting ledger instruction limits.
    /// Call repeatedly with the same max_count to drain remaining subscribers.
    /// Returns the total amount collected across all processed streams.
    pub fn withdraw_all(env: Env, creator: Address, max_count: u32) -> i128 {
        let creator_key = DataKey::CreatorSubscribers(creator.clone());
        let subs: Vec<Address> = env.storage().persistent()
            .get(&creator_key)
            .unwrap_or(vec![&env]);

        let mut total_collected: i128 = 0;
        let limit = max_count.min(subs.len()) as usize;

        for i in 0..limit {
            let subscriber = subs.get(i as u32).unwrap();
            let stream_key = DataKey::Stream(subscriber.clone(), creator.clone());

            if !env.storage().persistent().has(&stream_key) {
                continue;
            }

            let mut stream: Stream = env.storage().persistent().get(&stream_key).unwrap();
            let current_time = env.ledger().timestamp();

            if current_time <= stream.last_collected || stream.balance == 0 {
                continue;
            }

            let time_elapsed = (current_time - stream.last_collected) as i128;
            let mut claimable = time_elapsed * stream.rate_per_second;
            if claimable > stream.balance {
                claimable = stream.balance;
            }

            if claimable > 0 {
                total_collected += claimable;
                stream.balance -= claimable;
                stream.last_collected = current_time;
                env.storage().persistent().set(&stream_key, &stream);
            }
        }

        // Single transfer of the total collected amount to the creator
        if total_collected > 0 {
            // All streams share the same token — read it from the first valid stream
            for i in 0..limit {
                let subscriber = subs.get(i as u32).unwrap();
                let stream_key = DataKey::Stream(subscriber.clone(), creator.clone());
                if env.storage().persistent().has(&stream_key) {
                    let stream: Stream = env.storage().persistent().get(&stream_key).unwrap();
                    let token_client = TokenClient::new(&env, &stream.token);
                    token_client.transfer(&env.current_contract_address(), &creator, &total_collected);
                    break;
                }
            }
        }

        total_collected
    }
}

mod test;
