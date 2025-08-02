set shell := ["sh", "-c"]
set windows-shell := ["powershell.exe", "-NoLogo", "-Command"]
#set allow-duplicate-recipe
#set positional-arguments
set dotenv-filename := ".env"
set export


NEAR_WALLET:=env("NEAR_WALLET")
NEAR_WALLET_SEED:=env("NEAR_WALLET_SEED")

import "local.justfile"

deploy: 
  cargo near deploy build-non-reproducible-wasm {{NEAR_WALLET}} with-init-call new text-args "{}" prepaid-gas "100.0 Tgas" attached-deposit "0 NEAR" network-config testnet sign-with-seed-phrase "{{NEAR_WALLET_SEED}}" 