set shell := ["sh", "-c"]
set windows-shell := ["powershell.exe", "-NoLogo", "-Command"]
set dotenv-filename := ".env"
set export

# Environment variables from .env file
NEAR_WALLET := env("NEAR_WALLET")
NEAR_WALLET_SEED := env("NEAR_WALLET_SEED")
NETWORK := env_var_or_default("NEAR_NETWORK", "testnet")
GAS := env_var_or_default("NEAR_GAS", "100.0 Tgas")
DEPOSIT := env_var_or_default("NEAR_DEPOSIT", "1 NEAR")

# Subaccounts
TOKEN_ACCOUNT := "token." + NEAR_WALLET
HTLC_ACCOUNT := "htlc." + NEAR_WALLET
NEAR_TESTNET_WALLET := env("NEAR_TESTNET_WALLET", "https://wallet.meteorwallet.app")

# Import local configuration if available
import? "local.justfile"

# Common deploy command with all arguments
_deploy account_id="{{NEAR_WALLET}}":
    @echo "Deploying to {{account_id}}..."
    cargo near deploy build-non-reproducible-wasm {{account_id}} \
      with-init-call new text-args "{}" \
      prepaid-gas "{{GAS}}" \
      attached-deposit "{{DEPOSIT}}" \
      network-config {{NETWORK}} \
      sign-with-seed-phrase "{{NEAR_WALLET_SEED}}"

login: 
    @echo "Logging in..."
    near login

# Deploy main contract to primary account
deploy: 
    @just _deploy "{{NEAR_WALLET}}"

# Deploy token contract to token subaccount 
deploy-token:
    @just _deploy "{{TOKEN_ACCOUNT}}"

# Deploy HTLC contract to htlc subaccount
deploy-htlc:
    @just _deploy "{{HTLC_ACCOUNT}}"

# Create subaccounts if needed
create-subaccounts:
    @echo "Creating subaccounts if they don't exist..."
    near create-account {{TOKEN_ACCOUNT}} --masterAccount {{NEAR_WALLET}} --initialBalance 1 --useLedgerKey false
    near create-account {{HTLC_ACCOUNT}} --masterAccount {{NEAR_WALLET}} --initialBalance 1 --useLedgerKey false 

state account_id="{{NEAR_WALLET}}":
    @echo "Checking state for {{account_id}}..."
    near state {{account_id}}

states:
    @echo "Checking state..."
    near state {{NEAR_WALLET}}
    near state {{TOKEN_ACCOUNT}}
    near state {{HTLC_ACCOUNT}}