use near_sdk::borsh::{self, BorshDeserialize, BorshSerialize};
use near_sdk::{env, near_bindgen, AccountId};

#[near_bindgen]
#[derive(Default, BorshDeserialize, BorshSerialize)]
pub struct UnrealToken {
    // Optional fields if needed
}

#[near_bindgen]
impl UnrealToken {
    #[init]
    pub fn new() -> Self {
        Self {}
    }

    pub fn mint(&self, account: AccountId, amount: u128) {
        env::log_str(&format!("Mint called for {}: {}", account, amount));
        // Mint logic to be added here
    }
}
