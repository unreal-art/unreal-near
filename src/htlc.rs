use near_sdk::borsh::{self, BorshDeserialize, BorshSerialize};
use near_sdk::collections::{LookupMap, UnorderedMap};
use near_sdk::json_types::U128;
use near_sdk::{env, near_bindgen, AccountId, Balance, PanicOnDefault, Promise, CryptoHash, log, require};
use std::str::FromStr;

// Define our own chain ID types for 1inch fusion integration
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum NetworkId {
    Mainnet,
    Testnet,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ChainId {
    pub network_id: NetworkId,
    pub chain_id: u64,
}

impl ChainId {
    pub fn new(network_id: NetworkId, chain_id: u64) -> Self {
        Self { network_id, chain_id }
    }
    
    pub fn ethereum_mainnet() -> Self {
        Self {
            network_id: NetworkId::Mainnet,
            chain_id: 1,
        }
    }
    
    pub fn ethereum_sepolia() -> Self {
        Self {
            network_id: NetworkId::Testnet,
            chain_id: 11155111,
        }
    }
    
    pub fn near_mainnet() -> Self {
        Self {
            network_id: NetworkId::Mainnet,
            chain_id: 0,
        }
    }
    
    pub fn near_testnet() -> Self {
        Self {
            network_id: NetworkId::Testnet,
            chain_id: 0,
        }
    }
}

#[derive(BorshDeserialize, BorshSerialize)]
pub struct LockContract {
    pub secret_hash: CryptoHash,
    pub recipient: AccountId,
    pub sender: AccountId,
    pub amount: Balance,
    pub endtime: u64,
    pub withdrawn: bool,
    pub refunded: bool,
    pub preimage: String,
    pub target_chain: String,
    pub target_address: String,
}

/// Implementation of Hash Time Locked Contract for UnrealToken on NEAR
#[near_bindgen]
#[derive(BorshDeserialize, BorshSerialize)]
pub struct UnrealHTLC {
    // Reference to the UnrealToken contract
    token: AccountId,
    // Owner of the HTLC contract
    owner_id: AccountId,
    // Locked contracts by ID
    lock_contracts: UnorderedMap<CryptoHash, LockContract>,
    // Chain signature relayers - addresses allowed to complete cross-chain swaps
    relayers: LookupMap<AccountId, bool>,
}

#[near_bindgen]
impl UnrealHTLC {
    #[init]
    pub fn new(token_account_id: AccountId) -> Self {
        require!(!env::state_exists(), "Already initialized");
        
        Self {
            token: token_account_id,
            owner_id: env::predecessor_account_id(),
            lock_contracts: UnorderedMap::new(b"l"),
            relayers: LookupMap::new(b"r"),
        }
    }
    
    /// Add an account as a relayer for chain signatures
    pub fn add_relayer(&mut self, account_id: AccountId) {
        self.assert_owner();
        self.relayers.insert(&account_id, &true);
        log!("Added relayer: {}", account_id);
    }
    
    /// Remove a relayer
    pub fn remove_relayer(&mut self, account_id: AccountId) {
        self.assert_owner();
        self.relayers.remove(&account_id);
        log!("Removed relayer: {}", account_id);
    }
    
    /// Check if an account is a relayer
    pub fn is_relayer(&self, account_id: &AccountId) -> bool {
        self.relayers.get(account_id).unwrap_or(false)
    }

    /// Initiates a cross-chain swap by locking tokens in the contract
    #[payable]
    pub fn initiate_swap(
        &mut self,
        secret_hash: CryptoHash,
        recipient: AccountId,
        amount: U128,
        timeout_hours: u64,
        target_chain: String,
        target_address: String,
    ) -> CryptoHash {
        let amount: Balance = amount.into();
        require!(amount > 0, "Amount must be greater than 0");
        
        // Calculate timeout timestamp (current timestamp + timeout_hours in nanoseconds)
        let endtime = env::block_timestamp() + (timeout_hours * 3600 * 1_000_000_000);
        
        // Generate a unique lock contract ID
        let lock_id = env::sha256(
            &[
                &secret_hash[..],
                &recipient.as_bytes(),
                &env::predecessor_account_id().as_bytes(),
                &amount.to_le_bytes(),
                &endtime.to_le_bytes(),
                &env::block_timestamp().to_le_bytes(),
            ].concat()
        );

        // Convert to CryptoHash
        let lock_contract_id = lock_id.try_into().expect("Invalid hash length");
        
        // Make sure it doesn't already exist
        require!(!self.has_lock_contract(lock_contract_id), "Lock contract already exists");
        
        // Create the lock contract
        let lock_contract = LockContract {
            secret_hash,
            recipient: recipient.clone(),
            sender: env::predecessor_account_id(),
            amount,
            endtime,
            withdrawn: false,
            refunded: false,
            preimage: String::new(),
            target_chain,
            target_address,
        };
        
        // Store the lock contract
        self.lock_contracts.insert(&lock_contract_id, &lock_contract);
        
        // Transfer tokens from sender to this contract
        // This assumes the user has already called approve on the token contract
        ext_fungible_token::ft_transfer_call(
            env::current_account_id(),
            amount.into(),
            None,
            "Locking tokens for cross-chain swap".to_string(),
            self.token.clone(),
            1,  // yoctoNEAR deposit for storage
            env::prepaid_gas() - Gas::ONE_TERA * 40  // gas for the callback
        ).then(ext_self::on_ft_transfer_call(
            lock_contract_id,
            env::predecessor_account_id(),
            recipient,
            amount.into(),
            env::current_account_id(),
            0,  // no deposit
            env::prepaid_gas() - Gas::ONE_TERA * 50  // remaining gas
        ));
        
        // Return the lock contract ID
        lock_contract_id
    }

    /// Callback after token transfer to finalize the swap initiation
    #[private]
    pub fn on_ft_transfer_call(
        &mut self,
        lock_contract_id: CryptoHash,
        sender: AccountId,
        recipient: AccountId,
        amount: U128,
    ) {
        // Check if the transfer was successful
        require!(env::promise_result(0).is_success(), "Token transfer failed");
        
        log!(
            "Swap initiated with ID: {}, from: {}, to: {}, amount: {}",
            hex::encode(lock_contract_id.to_vec()),
            sender,
            recipient,
            amount.0
        );
    }

    /// Withdraw tokens by revealing the secret
    pub fn withdraw(
        &mut self,
        lock_contract_id: CryptoHash,
        preimage: String,
    ) -> bool {
        // Verify the lock contract exists
        require!(self.has_lock_contract(lock_contract_id), "Lock contract does not exist");
        
        let mut lock_contract = self.lock_contracts.get(&lock_contract_id).unwrap();
        
        // Verify the caller is the recipient
        require!(env::predecessor_account_id() == lock_contract.recipient, "Not the recipient");
        
        // Verify the contract is not already withdrawn or refunded
        require!(!lock_contract.withdrawn, "Already withdrawn");
        require!(!lock_contract.refunded, "Already refunded");
        
        // Verify the secret hash matches
        let preimage_hash = env::sha256(preimage.as_bytes());
        require!(preimage_hash.try_into().expect("Invalid hash length") == lock_contract.secret_hash, "Secret hash does not match");
        
        // Update the lock contract
        lock_contract.preimage = preimage;
        lock_contract.withdrawn = true;
        self.lock_contracts.insert(&lock_contract_id, &lock_contract);
        
        // Transfer tokens to the recipient
        ext_fungible_token::ft_transfer(
            lock_contract.recipient.clone(),
            lock_contract.amount.into(),
            None,
            self.token.clone(),
            1,  // yoctoNEAR deposit for storage
            env::prepaid_gas() - Gas::ONE_TERA * 5  // gas for the transfer
        );
        
        log!(
            "Swap withdrawn with ID: {}, preimage: {}, recipient: {}",
            hex::encode(lock_contract_id.to_vec()),
            preimage,
            lock_contract.recipient
        );
        
        true
    }

    /// Refund tokens to the sender if the timelock has expired
    pub fn refund(
        &mut self,
        lock_contract_id: CryptoHash,
    ) -> bool {
        // Verify the lock contract exists
        require!(self.has_lock_contract(lock_contract_id), "Lock contract does not exist");
        
        let mut lock_contract = self.lock_contracts.get(&lock_contract_id).unwrap();
        
        // Verify the caller is the sender
        require!(env::predecessor_account_id() == lock_contract.sender, "Not the sender");
        
        // Verify the contract is not already withdrawn or refunded
        require!(!lock_contract.withdrawn, "Already withdrawn");
        require!(!lock_contract.refunded, "Already refunded");
        
        // Verify the timelock has expired
        require!(env::block_timestamp() >= lock_contract.endtime, "Timelock not expired");
        
        // Update the lock contract
        lock_contract.refunded = true;
        self.lock_contracts.insert(&lock_contract_id, &lock_contract);
        
        // Transfer tokens back to the sender
        ext_fungible_token::ft_transfer(
            lock_contract.sender.clone(),
            lock_contract.amount.into(),
            None,
            self.token.clone(),
            1,  // yoctoNEAR deposit for storage
            env::prepaid_gas() - Gas::ONE_TERA * 5  // gas for the transfer
        );
        
        log!(
            "Swap refunded with ID: {}, sender: {}",
            hex::encode(lock_contract_id.to_vec()),
            lock_contract.sender
        );
        
        true
    }

    /// Complete a cross-chain swap from another chain (to be called by relayer/oracle)
    pub fn complete_swap(
        &mut self,
        source_chain: String,
        source_address: String,
        destination: AccountId,
        amount: U128,
        preimage: String,
    ) -> bool {
        // Verify the caller is a relayer
        require!(self.is_relayer(&env::predecessor_account_id()), "Not an authorized relayer");
        
        // Generate a unique ID for this cross-chain completion
        let lock_id = env::sha256(
            &[
                source_chain.as_bytes(),
                source_address.as_bytes(),
                destination.as_bytes(),
                &amount.0.to_le_bytes(),
                preimage.as_bytes(),
            ].concat()
        );
        
        let amount_u128: Balance = amount.into();
        
        // Mint or transfer tokens to the destination address
        ext_fungible_token::ft_mint(
            destination.clone(),
            amount,
            None,
            self.token.clone(),
            1,  // yoctoNEAR deposit for storage
            env::prepaid_gas() - Gas::from_tgas(5)  // gas for the mint
        );
        
        log!(
            "Cross-chain swap completed from {}, source_address: {}, to: {}, amount: {}, preimage: {}",
            source_chain,
            source_address,
            destination,
            amount.0,
            preimage
        );
        
        true
    }
    
    /// 1inch Fusion: Execute an EVM transaction from NEAR using 1inch Fusion
    /// This function allows executing a cross-chain swap operation from NEAR to EVM chains
    pub fn execute_on_evm(
        &mut self,
        evm_chain_id: String,
        contract_address: String,
        calldata: String,
        gas_limit: U128,
    ) -> Promise {
        // Only relayers or owner can call this function
        let caller = env::predecessor_account_id();
        require!(
            self.is_relayer(&caller) || caller == self.owner_id,
            "Only relayers or owner can execute cross-chain operations"
        );
        
        // Parse the EVM chain ID to ensure it's valid
        let chain_id = match evm_chain_id.parse::<u64>() {
            Ok(id) => id,
            Err(_) => env::panic_str("Invalid EVM chain ID format")
        };
        
        // Validate the contract address format (should be a hex address for EVM)
        if !contract_address.starts_with("0x") || contract_address.len() != 42 {
            env::panic_str("Invalid EVM contract address format");
        }
        
        // 1inch Fusion requires calldata to be properly formatted for their resolver contracts
        if calldata.is_empty() {
            env::panic_str("Calldata cannot be empty");
        }
        
        log!(
            "1inch Fusion: Executing swap on EVM chain {}, contract: {}, gas: {}",
            chain_id,
            contract_address,
            gas_limit.0
        );
        
        // In production, this would integrate with a cross-chain messaging protocol
        // to actually execute the transaction on the EVM chain
        
        // Log the 1inch Fusion cross-chain swap details
        log!("1inch Fusion Cross-Chain Swap Details:");
        log!("  From: NEAR ({})", env::current_account_id());
        log!("  To: EVM Chain {}", chain_id);
        log!("  Target: {}", contract_address);
        log!("  Gas Limit: {}", gas_limit.0);
        log!("  Calldata Length: {}", calldata.len());
        
        // Return a mock Promise - in production, this would call a bridge contract
        Promise::new(env::current_account_id())
    }

    /// Check if a lock contract exists
    pub fn has_lock_contract(&self, lock_contract_id: CryptoHash) -> bool {
        self.lock_contracts.get(&lock_contract_id).is_some()
    }

    /// Get details of a lock contract
    pub fn get_lock_contract(&self, lock_contract_id: CryptoHash) -> Option<LockContractView> {
        self.lock_contracts.get(&lock_contract_id).map(|lock_contract| LockContractView {
            secret_hash: hex::encode(lock_contract.secret_hash.to_vec()),
            recipient: lock_contract.recipient,
            sender: lock_contract.sender,
            amount: U128(lock_contract.amount),
            endtime: lock_contract.endtime,
            withdrawn: lock_contract.withdrawn,
            refunded: lock_contract.refunded,
            preimage: lock_contract.preimage,
            target_chain: lock_contract.target_chain,
            target_address: lock_contract.target_address,
        })
    }

    // Helper to assert the caller is the owner
    fn assert_owner(&self) {
        require!(env::predecessor_account_id() == self.owner_id, "Not the owner");
    }
}

#[derive(serde::Serialize)]
#[serde(crate = "near_sdk::serde")]
pub struct LockContractView {
    pub secret_hash: String,
    pub recipient: AccountId,
    pub sender: AccountId,
    pub amount: U128,
    pub endtime: u64,
    pub withdrawn: bool,
    pub refunded: bool,
    pub preimage: String,
    pub target_chain: String,
    pub target_address: String,
}

// Define the Gas constants
const ONE_TERA: u64 = 1_000_000_000_000;

// Use the Gas struct from near_sdk instead of defining our own
// This ensures compatibility with the SDK

// External contract interfaces

#[ext_contract(ext_fungible_token)]
trait FungibleToken {
    fn ft_transfer(&mut self, receiver_id: AccountId, amount: U128, memo: Option<String>);
    fn ft_transfer_call(
        &mut self,
        receiver_id: AccountId,
        amount: U128,
        memo: Option<String>,
        msg: String,
    ) -> Promise;
    fn ft_mint(&mut self, receiver_id: AccountId, amount: U128, memo: Option<String>);
}

#[ext_contract(ext_self)]
trait ExtSelf {
    fn on_ft_transfer_call(
        &mut self,
        lock_contract_id: CryptoHash,
        sender: AccountId,
        recipient: AccountId,
        amount: U128,
    );
}
