use near_sdk::borsh::{BorshDeserialize, BorshSerialize};
use near_sdk::collections::{LookupMap, LazyOption};
use near_sdk::{env, near_bindgen, AccountId, PanicOnDefault, Gas, log};
use near_sdk::json_types::U128;
use std::collections::HashMap;

type Balance = u128;

/// Constants for gas and storage
const TGAS: u64 = 1_000_000_000_000;
const GAS_FOR_FT_TRANSFER: Gas = Gas::from_tgas(5);
const GAS_FOR_RESOLVE_TRANSFER: Gas = Gas::from_tgas(10);
/// Initial balance for the FT contract itself
const CONTRACT_STORAGE_COST: Balance = 10_000_000_000_000_000_000_000; // 0.01 NEAR

/// The following is the NEP-141 standard for fungible tokens on NEAR
/// It's equivalent to ERC-20 on Ethereum

#[near_bindgen]
#[derive(BorshDeserialize, BorshSerialize, PanicOnDefault)]
pub struct UnrealToken {
    /// Name of the token
    name: String,
    /// Symbol of the token
    symbol: String,
    /// Total supply of the token
    total_supply: Balance,
    /// Decimals for the token
    decimals: u8,
    /// Owner of the contract with admin rights
    owner_id: AccountId,
    /// Contract pause state
    paused: bool,
    /// Balances of each account
    balances: LookupMap<AccountId, Balance>,
    /// Allowances between accounts (from, to) -> amount
    allowances: LookupMap<AccountId, HashMap<AccountId, Balance>>,
    /// Metadata for the contract itself
    metadata: LazyOption<FungibleTokenMetadata>,
}

#[derive(BorshDeserialize, BorshSerialize)]
pub struct FungibleTokenMetadata {
    pub name: String,
    pub symbol: String,
    pub decimals: u8,
}

#[near_bindgen]
impl UnrealToken {
    /// Initializes the contract with name, symbol, and decimals
    #[init]
    pub fn new(
        name: String, 
        symbol: String, 
        decimals: u8,
        initial_supply: U128
    ) -> Self {
        // Ensure contract is not initialized yet
        assert!(!env::state_exists(), "Contract is already initialized");
        let owner_id = env::predecessor_account_id();
        let mut this = Self {
            name: name.clone(),
            symbol: symbol.clone(),
            total_supply: initial_supply.into(),
            decimals,
            owner_id: owner_id.clone(),
            paused: false,
            balances: LookupMap::new(b"b"),
            allowances: LookupMap::new(b"a"),
            metadata: LazyOption::new(
                b"m", 
                Some(&FungibleTokenMetadata {
                    name: name.clone(),
                    symbol: symbol.clone(),
                    decimals,
                }),
            ),
        };
        
        // Mint the initial supply to the contract owner
        this.internal_deposit(&owner_id, initial_supply.into());
        log!("Initialized token with {} supply to {}", initial_supply.0, owner_id);
        
        this
    }

    /****************************************
    * Basic NEP-141 implementation (ERC-20) *
    *****************************************/
    
    /// Returns the name of the token
    pub fn name(&self) -> String {
        self.name.clone()
    }
    
    /// Returns the symbol of the token
    pub fn symbol(&self) -> String {
        self.symbol.clone()
    }
    
    /// Returns the decimals of the token
    pub fn decimals(&self) -> u8 {
        self.decimals
    }
    
    /// Returns the total supply of the token
    pub fn total_supply(&self) -> U128 {
        U128(self.total_supply)
    }

    /// Returns the balance of the specified account
    pub fn balance_of(&self, account_id: AccountId) -> U128 {
        U128(self.balances.get(&account_id).unwrap_or(0))
    }
    
    /// Returns the allowance of the `spender` for the `owner`
    pub fn allowance(&self, owner_id: AccountId, spender_id: AccountId) -> U128 {
        self.internal_get_allowance(&owner_id, &spender_id)
    }

    /// Transfer tokens to a specified account
    pub fn transfer(&mut self, receiver_id: AccountId, amount: U128) -> bool {
        self.assert_not_paused();
        self.internal_transfer(
            &env::predecessor_account_id(),
            &receiver_id,
            amount.into(),
            None,
        );
        true
    }

    /// Transfer tokens from a specified account (if approved)
    pub fn transfer_from(&mut self, sender_id: AccountId, receiver_id: AccountId, amount: U128) -> bool {
        self.assert_not_paused();
        let caller_id = env::predecessor_account_id();
        let amount_u128: Balance = amount.into();
        self.internal_decrease_allowance(&sender_id, &caller_id, amount_u128);
        self.internal_transfer(&sender_id, &receiver_id, amount_u128, None);
        true
    }

    /// Approve `spender` to transfer tokens on behalf of the caller
    pub fn approve(&mut self, spender_id: AccountId, amount: U128) -> bool {
        self.assert_not_paused();
        self.internal_approve(
            &env::predecessor_account_id(),
            &spender_id,
            amount.into(),
        )
    }

    /********************************
    * Owner Management & Pausable  *
    ********************************/

    /// Returns true if the contract is currently paused
    pub fn is_paused(&self) -> bool {
        self.paused
    }
    
    /// Returns the account ID of the contract owner
    pub fn owner_id(&self) -> AccountId {
        self.owner_id.clone()
    }
    
    /// Pause the contract - only callable by owner
    pub fn pause(&mut self) {
        self.assert_owner();
        self.paused = true;
        log!("Contract paused by owner");
    }
    
    /// Unpause the contract - only callable by owner
    pub fn unpause(&mut self) {
        self.assert_owner();
        self.paused = false;
        log!("Contract unpaused by owner");
    }
    
    /// Transfer ownership to new account - only callable by owner
    pub fn transfer_ownership(&mut self, new_owner: AccountId) {
        self.assert_owner();
        self.owner_id = new_owner.clone();
        log!("Ownership transferred to {}", new_owner);
    }

    /***********************
    * Minting and Burning *
    ***********************/

    /// Mint tokens to specified account - only callable by owner
    pub fn mint(&mut self, to: AccountId, amount: U128) {
        self.assert_owner();
        self.assert_not_paused();
        let amount_u128: Balance = amount.into();
        self.internal_deposit(&to, amount_u128);
        self.total_supply += amount_u128;
        log!("Minted {} tokens to {}", amount.0, to);
    }

    /// Burn tokens from specified account - only callable by owner
    pub fn burn(&mut self, from: AccountId, amount: U128) {
        self.assert_owner();
        self.assert_not_paused();
        let amount_u128: Balance = amount.into();
        self.internal_withdraw(&from, amount_u128);
        self.total_supply -= amount_u128;
        log!("Burned {} tokens from {}", amount.0, from);
    }

    /*************************
    * Internal Helper Methods *
    *************************/

    /// Assert that the caller is the contract owner
    fn assert_owner(&self) {
        assert_eq!(
            env::predecessor_account_id(),
            self.owner_id,
            "Only the owner can call this method"
        );
    }

    /// Assert that the contract is not paused
    fn assert_not_paused(&self) {
        assert!(!self.paused, "Contract is paused");
    }

    /// Internal implementation of deposit to an account
    fn internal_deposit(&mut self, account_id: &AccountId, amount: Balance) {
        let balance = self.balances.get(&account_id).unwrap_or(0);
        self.balances.insert(&account_id, &(balance + amount));
    }

    /// Internal implementation of withdraw from an account
    fn internal_withdraw(&mut self, account_id: &AccountId, amount: Balance) {
        let balance = self.balances.get(&account_id).unwrap_or(0);
        assert!(balance >= amount, "Insufficient balance");
        self.balances.insert(&account_id, &(balance - amount));
    }

    /// Internal implementation of transfer between accounts
    fn internal_transfer(
        &mut self,
        sender_id: &AccountId,
        receiver_id: &AccountId,
        amount: Balance,
        memo: Option<String>,
    ) {
        assert_ne!(sender_id, receiver_id, "Cannot transfer to yourself");
        assert!(amount > 0, "The amount should be a positive number");
        self.internal_withdraw(sender_id, amount);
        self.internal_deposit(receiver_id, amount);
        if let Some(memo_text) = memo {
            log!("Memo: {}", memo_text);
        }
        log!("Transfer {} from {} to {}", amount, sender_id, receiver_id);
    }

    /// Internal implementation of getting allowance
    fn internal_get_allowance(&self, owner_id: &AccountId, spender_id: &AccountId) -> U128 {
        match self.allowances.get(&owner_id) {
            Some(allowances) => U128(allowances.get(spender_id).cloned().unwrap_or(0)),
            None => U128(0),
        }
    }

    /// Internal implementation of approving allowance
    fn internal_approve(
        &mut self,
        owner_id: &AccountId,
        spender_id: &AccountId,
        amount: Balance,
    ) -> bool {
        let mut allowances = self.allowances.get(&owner_id).unwrap_or_else(|| HashMap::new());
        allowances.insert(spender_id.clone(), amount);
        self.allowances.insert(&owner_id, &allowances);
        log!(
            "Approval: Owner: {} approved Spender: {} to use {} tokens",
            owner_id, spender_id, amount
        );
        true
    }

    /// Internal implementation of decreasing allowance
    fn internal_decrease_allowance(
        &mut self,
        owner_id: &AccountId,
        spender_id: &AccountId,
        amount: Balance,
    ) {
        let allowance = self.internal_get_allowance(owner_id, spender_id).0;
        assert!(allowance >= amount, "Insufficient allowance");
        let mut allowances = self.allowances.get(&owner_id).unwrap_or_else(|| HashMap::new());
        allowances.insert(spender_id.clone(), allowance - amount);
        self.allowances.insert(&owner_id, &allowances);
    }
}
