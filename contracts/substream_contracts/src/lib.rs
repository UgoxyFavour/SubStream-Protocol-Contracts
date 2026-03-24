#![no_std]
use soroban_sdk::token::Client as TokenClient;
use soroban_sdk::{contract, contractimpl, contracttype, Address, Env};

#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum DataKey {
    Stream(Address, Address),        // (subscriber, creator)
    TotalStreamed(Address, Address), // (subscriber, creator) - cumulative tokens streamed
    CliffThreshold(Address),         // creator -> threshold amount for access
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
            token_client.transfer(
                &env.current_contract_address(),
                &creator,
                &amount_to_collect,
            );

            stream.balance -= amount_to_collect;
            stream.last_collected = current_time;

            env.storage().persistent().set(&key, &stream);
            Self::update_total_streamed(&env, &subscriber, &creator, amount_to_collect);
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
            token_client.transfer(
                &env.current_contract_address(),
                &subscriber,
                &stream.balance,
            );
        }

        // Remove the stream from storage
        env.storage().persistent().remove(&key);
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

    pub fn set_cliff_threshold(env: Env, creator: Address, threshold: i128) {
        creator.require_auth();
        if threshold < 0 {
            panic!("threshold must be non-negative");
        }
        let key = DataKey::CliffThreshold(creator.clone());
        env.storage().persistent().set(&key, &threshold);
    }

    pub fn get_cliff_threshold(env: Env, creator: Address) -> i128 {
        let key = DataKey::CliffThreshold(creator.clone());
        env.storage().persistent().get(&key).unwrap_or(0)
    }

    pub fn get_total_streamed(env: Env, subscriber: Address, creator: Address) -> i128 {
        let key = DataKey::TotalStreamed(subscriber.clone(), creator.clone());
        env.storage().persistent().get(&key).unwrap_or(0)
    }

    pub fn has_unlocked_access(env: Env, subscriber: Address, creator: Address) -> bool {
        let threshold_key = DataKey::CliffThreshold(creator.clone());
        let threshold: i128 = env.storage().persistent().get(&threshold_key).unwrap_or(0);
        if threshold == 0 {
            return true;
        }
        let streamed_key = DataKey::TotalStreamed(subscriber.clone(), creator.clone());
        let total_streamed: i128 = env.storage().persistent().get(&streamed_key).unwrap_or(0);
        total_streamed >= threshold
    }

    pub fn get_access_tier(env: Env, subscriber: Address, creator: Address) -> u32 {
        let threshold_key = DataKey::CliffThreshold(creator.clone());
        let threshold: i128 = env.storage().persistent().get(&threshold_key).unwrap_or(0);
        if threshold == 0 {
            return 2;
        }
        let streamed_key = DataKey::TotalStreamed(subscriber.clone(), creator.clone());
        let total_streamed: i128 = env.storage().persistent().get(&streamed_key).unwrap_or(0);

        if total_streamed >= 500 {
            3
        } else if total_streamed >= 200 {
            2
        } else if total_streamed >= 50 {
            1
        } else {
            0
        }
    }

    fn update_total_streamed(env: &Env, subscriber: &Address, creator: &Address, amount: i128) {
        let key = DataKey::TotalStreamed(subscriber.clone(), creator.clone());
        let current_total: i128 = env.storage().persistent().get(&key).unwrap_or(0);
        let new_total = current_total + amount;
        env.storage().persistent().set(&key, &new_total);
    }
}

mod test;
