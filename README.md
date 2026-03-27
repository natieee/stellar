# soroban-rental

An NFT rental protocol on Soroban (Stellar's smart contract platform) where owners lock an asset and grant temporary usage rights to a renter for a configurable period and fee. Renters receive a time-limited access credential. Assets automatically return to owners after the rental period without requiring any on-chain action.

Designed for in-game items, event tickets, and access passes.

---

## Overview

`soroban-rental` handles the full lifecycle of an asset rental on-chain — from listing and fee collection, through time-limited credential issuance, to automatic expiry and re-availability.

All rental fees flow into a contract escrow on payment. The platform takes a 2.5% cut immediately. The owner's proceeds are held in escrow and released either on normal expiry or pro-rated on early termination — no owner authorization required for refunds since the contract holds the funds.

---

## Features

- ✅ Owner lists any asset (identified by `asset_contract + token_id`) with configurable fee and duration
- ✅ Renter pays fee and receives a `RentalCredential` valid until `expires_at`
- ✅ Credential automatically invalid after expiry — no on-chain action required
- ✅ Escrow model — contract holds owner proceeds, no owner auth needed for refunds
- ✅ Platform fee: 2.5% of rental fee forwarded immediately to fee recipient
- ✅ Pro-rated early termination — renter exits early, both parties receive fair share
- ✅ `expire_rental` — permissionless call resets listing to Available after expiry
- ✅ Re-rentable — listing becomes available again after each rental ends
- ✅ Free listings supported (fee = 0) — credential still issued
- ✅ Global listing index for enumeration
- ✅ `quote_rental` — preview total cost before committing

---

## Escrow Model
```
rent()
 ├── Renter pays full fee → contract escrow
 ├── Platform fee (2.5%) → fee_recipient immediately
 └── Owner proceeds → held in escrow

expire_rental() [normal end, anyone can call]
 └── Full escrow → owner

terminate_early() [renter exits before expiry]
 ├── escrow × (time_used / total_duration) → owner
 └── escrow × (time_remaining / total_duration) → renter refund
```

---

## Contract Interface
```rust
// Setup
fn initialize(env: Env, admin: Address, fee_recipient: Address);

// Owner: Listing Management
fn list_asset(
    env: Env,
    owner: Address,
    asset_contract: Address,
    token_id: u64,
    payment_token: Address,
    fee_per_period: i128,
    period_duration: u64,
    max_periods: u32,
);
fn update_listing(
    env: Env,
    owner: Address,
    asset_contract: Address,
    token_id: u64,
    new_fee_per_period: i128,
    new_period_duration: u64,
    new_max_periods: u32,
);
fn delist_asset(env: Env, owner: Address, asset_contract: Address, token_id: u64);

// Renter: Rental Flow
fn rent(env: Env, renter: Address, asset_contract: Address, token_id: u64, periods: u32);
fn terminate_early(env: Env, renter: Address, asset_contract: Address, token_id: u64);

// Lifecycle
fn expire_rental(env: Env, asset_contract: Address, token_id: u64);  // permissionless

// Validation
fn is_credential_valid(env: Env, renter: Address, asset_contract: Address, token_id: u64) -> bool;
fn is_credential_expired(env: Env, asset_contract: Address, token_id: u64) -> bool;

// Queries
fn get_listing(env: Env, asset_contract: Address, token_id: u64) -> Listing;
fn get_credential(env: Env, asset_contract: Address, token_id: u64) -> RentalCredential;
fn get_all_listings(env: Env) -> Vec<ListingKey>;
fn quote_rental(env: Env, asset_contract: Address, token_id: u64, periods: u32) -> i128;
fn get_admin(env: Env) -> Address;
fn get_fee_recipient(env: Env) -> Address;
fn set_fee_recipient(env: Env, admin: Address, new_recipient: Address);
```

---

## Data Structures
```rust
pub struct Listing {
    pub asset_contract: Address,
    pub token_id: u64,
    pub owner: Address,
    pub payment_token: Address,
    pub fee_per_period: i128,
    pub period_duration: u64,   // seconds per rental period (e.g. 86400 = 1 day)
    pub max_periods: u32,       // max periods a single renter can book (1–365)
    pub status: ListingStatus,  // Available | Rented | Delisted
    pub created_at: u64,
}

pub struct RentalCredential {
    pub asset_contract: Address,
    pub token_id: u64,
    pub renter: Address,
    pub owner: Address,
    pub started_at: u64,
    pub expires_at: u64,
    pub periods: u32,
    pub total_fee_paid: i128,   // gross fee paid by renter
    pub escrow_amount: i128,    // owner proceeds held in contract escrow
    pub terminated_early: bool,
}
```

---

## Storage Schema

| Key | Type | Description |
|-----|------|-------------|
| `ADMIN` | `Address` | Contract administrator |
| `FeeRecipient` | `Address` | Platform fee recipient |
| `ListingIndex` | `Vec<ListingKey>` | Global listing enumeration index |
| `Listing(contract, id)` | `Listing` | Listing state per asset |
| `Credential(contract, id)` | `RentalCredential` | Active credential per asset |

---

## Fee Structure

| Party | Amount | When |
|-------|--------|------|
| Platform | 2.5% of total fee | Immediately at `rent()` |
| Owner | 97.5% of total fee | At `expire_rental()` or pro-rated at `terminate_early()` |
| Renter refund | Pro-rated unused portion | At `terminate_early()` only |

Platform fee is configurable via `set_fee_recipient`. The 2.5% rate (`PLATFORM_FEE_BPS = 250`) is a compile-time constant.

---

## Rental Lifecycle
```
initialize()
     │
list_asset()                     ← owner lists asset
     │
     ├─ update_listing()         ← owner can adjust terms (Available only)
     │
rent()                           ← renter pays, credential issued
     │
     ├─ [during rental]
     │    ├─ is_credential_valid()   ← dApps verify access
     │    └─ terminate_early()       ← renter exits early, pro-rated refund
     │
     └─ [after expires_at]
          ├─ is_credential_expired() ← returns true automatically
          ├─ expire_rental()         ← anyone resets listing to Available
          └─ rent()                  ← next renter can book
```

---

## Listing Constraints

| Parameter | Constraint | Notes |
|-----------|-----------|-------|
| `fee_per_period` | `>= 0` | 0 = free listing |
| `period_duration` | `> 0` seconds | e.g. 3600 = 1hr, 86400 = 1 day |
| `max_periods` | `1–365` | Cap per single rental |
| `periods` at rent | `1 ≤ periods ≤ max_periods` | Renter chooses duration |

---

## Project Structure
```
soroban-rental/
├── Cargo.toml
└── src/
    ├── lib.rs       # Contract implementation
    └── test.rs      # Full test suite (34 tests)
```

---

## Getting Started

### Prerequisites
```bash
rustup target add wasm32-unknown-unknown
cargo install --locked stellar-cli --features opt
```

### Run Tests
```bash
cargo test --features testutils
```

### Build
```bash
stellar contract build
```

Output: `target/wasm32-unknown-unknown/release/soroban_rental.wasm`

---

## How to Deploy

### Step 1 — Generate Identities
```bash
stellar keys generate --global admin --network testnet
stellar keys fund admin --network testnet

stellar keys generate --global fee-wallet --network testnet
```

### Step 2 — Deploy
```bash
stellar contract deploy \
  --wasm target/wasm32-unknown-unknown/release/soroban_rental.wasm \
  --source admin \
  --network testnet
```

Save the returned Contract ID:
```
Contract ID: CBY4SG7HRK6S2ZS7FI423FB3FB5R2I4WLD2KRFSKUBH56SFDHHL7XAG2
🔗 https://stellar.expert/explorer/testnet/tx/1a622e1541d8f385ff331ce4582bac3306c9e60aa1140945889c4d0369bc03f1
🔗 https://lab.stellar.org/r/testnet/contract/CBY4SG7HRK6S2ZS7FI423FB3FB5R2I4WLD2KRFSKUBH56SFDHHL7XAG2
```

### Step 3 — Initialize
```bash
stellar contract invoke \
  --id <CONTRACT_ID> \
  --source admin \
  --network testnet \
  -- \
  initialize \
  --admin <ADMIN_ADDRESS> \
  --fee_recipient <FEE_WALLET_ADDRESS>
```

### Step 4 — Owner Lists an Asset
```bash
stellar contract invoke \
  --id <CONTRACT_ID> \
  --source owner \
  --network testnet \
  -- \
  list_asset \
  --owner <OWNER_ADDRESS> \
  --asset_contract <NFT_CONTRACT_ADDRESS> \
  --token_id 42 \
  --payment_token <TOKEN_CONTRACT_ADDRESS> \
  --fee_per_period 1000000 \
  --period_duration 86400 \
  --max_periods 30
```

### Step 5 — Renter Previews Cost
```bash
stellar contract invoke \
  --id <CONTRACT_ID> \
  --source renter \
  --network testnet \
  -- \
  quote_rental \
  --asset_contract <NFT_CONTRACT_ADDRESS> \
  --token_id 42 \
  --periods 7
```

### Step 6 — Renter Books the Asset
```bash
stellar contract invoke \
  --id <CONTRACT_ID> \
  --source renter \
  --network testnet \
  -- \
  rent \
  --renter <RENTER_ADDRESS> \
  --asset_contract <NFT_CONTRACT_ADDRESS> \
  --token_id 42 \
  --periods 7
```

### Step 7 — Verify Credential (dApp Integration)
```bash
stellar contract invoke \
  --id <CONTRACT_ID> \
  --source renter \
  --network testnet \
  -- \
  is_credential_valid \
  --renter <RENTER_ADDRESS> \
  --asset_contract <NFT_CONTRACT_ADDRESS> \
  --token_id 42
```

### Step 8 — Expire and Re-list (after rental ends)
```bash
stellar contract invoke \
  --id <CONTRACT_ID> \
  --source anyone \
  --network testnet \
  -- \
  expire_rental \
  --asset_contract <NFT_CONTRACT_ADDRESS> \
  --token_id 42
```

### Step 9 — Early Termination (renter exits before expiry)
```bash
stellar contract invoke \
  --id <CONTRACT_ID> \
  --source renter \
  --network testnet \
  -- \
  terminate_early \
  --renter <RENTER_ADDRESS> \
  --asset_contract <NFT_CONTRACT_ADDRESS> \
  --token_id 42
```

### Step 10 — Deploy to Mainnet
```bash
stellar contract deploy \
  --wasm target/wasm32-unknown-unknown/release/soroban_rental.wasm \
  --source admin \
  --network mainnet
```

---

## Network RPC Endpoints

| Network | RPC URL |
|---------|---------|
| Testnet | `https://soroban-testnet.stellar.org` |
| Mainnet | `https://mainnet.stellar.validationcloud.io/v1/<API_KEY>` |

---

## Design Notes

**Why escrow instead of direct owner payment?**
Soroban's auth model forbids a contract from initiating a token transfer out of an address that hasn't explicitly authorized that sub-call in the current invocation. Holding owner proceeds in contract escrow means all refund and payout flows originate from the contract's own balance — no owner re-authorization needed at termination time.

**Why is `expire_rental` permissionless?**
After `expires_at` passes, the credential is already invalid — no further access is possible. Making `expire_rental` permissionless allows anyone (the owner, the renter, a keeper bot, or the next renter) to reset the listing to Available without the original owner having to be online. This maximizes asset utilization.

**Why `token_id` as `u64` rather than a string?**
Numeric token IDs are the most common NFT identifier pattern on Stellar and keep storage costs low. String-based IDs can be layered on top by the asset contract itself.

**Why 1–365 max_periods?**
Unbounded rental periods create UX risk (a renter accidentally booking 10,000 days). The 365-day cap is a sensible default for access passes and game items. Owners can set `max_periods = 1` for single-period only.

---

## Tech Stack

- **Soroban SDK** `21.0.0` — Stellar smart contract runtime
- **Stellar Token Interface** — SAC-compatible token transfers and escrow
- **Rust** — Contract implementation
- **Stellar CLI** — Build, deploy, and invoke tooling

---

## License

MIT
