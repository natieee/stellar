#![no_std]

//! # soroban-rental
//!
//! An NFT rental protocol where owners lock an asset and grant temporary
//! usage rights to a renter for a configurable period and fee.
//!
//! ## Core Design
//!
//! - Owner lists an asset (identified by contract + token_id) with a rental fee
//!   and maximum duration.
//! - Renter pays the fee and receives a time-limited `RentalCredential`.
//! - The credential is valid until `expires_at` (ledger timestamp).
//! - After expiry the credential is invalid — no on-chain action required.
//! - Owner can reclaim the listing at any time after expiry.
//! - Early termination is supported: renter returns early, owner receives
//!   pro-rated fee, renter receives a pro-rated refund.

use soroban_sdk::{
    contract, contractimpl, contracttype, symbol_short, token, Address, Env, Symbol, Vec,
};

// ─────────────────────────────────────────────
// Constants
// ─────────────────────────────────────────────

const ADMIN_KEY: Symbol = symbol_short!("ADMIN");

/// Basis points denominator
const BPS_DENOM: i128 = 10_000;

/// Platform fee in basis points (2.5%)
const PLATFORM_FEE_BPS: i128 = 250;

// ─────────────────────────────────────────────
// Storage Keys
// ─────────────────────────────────────────────

#[contracttype]
#[derive(Clone)]
pub enum DataKey {
    /// Listing keyed by (asset_contract, token_id)
    Listing(Address, u64),
    /// Active rental credential keyed by (asset_contract, token_id)
    Credential(Address, u64),
    /// All listing keys for enumeration
    ListingIndex,
    /// Platform fee recipient
    FeeRecipient,
}

// ─────────────────────────────────────────────
// Enums
// ─────────────────────────────────────────────

#[contracttype]
#[derive(Clone, Debug, PartialEq)]
pub enum ListingStatus {
    /// Available for rent
    Available,
    /// Currently rented out
    Rented,
    /// Delisted by owner
    Delisted,
}

// ─────────────────────────────────────────────
// Data Structures
// ─────────────────────────────────────────────

/// A listing created by an NFT owner making their asset available for rent.
#[contracttype]
#[derive(Clone, Debug)]
pub struct Listing {
    /// The NFT/asset contract address
    pub asset_contract: Address,
    /// Token ID within the asset contract
    pub token_id: u64,
    /// Owner who created the listing
    pub owner: Address,
    /// Payment token used for the rental fee
    pub payment_token: Address,
    /// Fee per rental period unit (in payment_token base units)
    pub fee_per_period: i128,
    /// Length of one rental period in seconds
    pub period_duration: u64,
    /// Maximum number of periods a renter can rent for
    pub max_periods: u32,
    /// Current listing status
    pub status: ListingStatus,
    /// Timestamp when listing was created
    pub created_at: u64,
}

/// A time-limited rental credential granted to a renter.
#[contracttype]
#[derive(Clone, Debug)]
pub struct RentalCredential {
    /// The NFT/asset contract address
    pub asset_contract: Address,
    /// Token ID being rented
    pub token_id: u64,
    /// Address of the renter holding this credential
    pub renter: Address,
    /// Address of the asset owner
    pub owner: Address,
    /// Ledger timestamp when rental started
    pub started_at: u64,
    /// Ledger timestamp when rental expires
    pub expires_at: u64,
    /// Number of periods rented
    pub periods: u32,
    /// Gross fee paid by renter
    pub total_fee_paid: i128,
    /// Owner proceeds held in contract escrow (total_fee - platform_fee)
    pub escrow_amount: i128,
    /// Whether this rental has been explicitly terminated early
    pub terminated_early: bool,
}

/// Summary entry used for listing index enumeration.
#[contracttype]
#[derive(Clone, Debug)]
pub struct ListingKey {
    pub asset_contract: Address,
    pub token_id: u64,
}

// ─────────────────────────────────────────────
// Contract
// ─────────────────────────────────────────────

#[contract]
pub struct RentalContract;

#[contractimpl]
impl RentalContract {
    // ── Initialization ───────────────────────

    /// Initialize contract with an admin and platform fee recipient.
    pub fn initialize(env: Env, admin: Address, fee_recipient: Address) {
        if env.storage().instance().has(&ADMIN_KEY) {
            panic!("already initialized");
        }
        env.storage().instance().set(&ADMIN_KEY, &admin);
        env.storage()
            .instance()
            .set(&DataKey::FeeRecipient, &fee_recipient);
        env.storage()
            .instance()
            .set(&DataKey::ListingIndex, &Vec::<ListingKey>::new(&env));
    }

    // ── Owner: Listing Management ────────────

    /// Create a new rental listing for an asset.
    ///
    /// The owner must hold the asset (verified off-chain or via integration).
    /// The contract does not custody the NFT itself — it issues credentials.
    /// This design is intentional: Soroban NFT standards vary; the rental
    /// protocol focuses on access credential issuance, not asset custody.
    ///
    /// - `fee_per_period`: payment token units charged per period
    /// - `period_duration`: seconds per rental period (e.g. 86400 = 1 day)
    /// - `max_periods`: maximum periods a single renter can book
    pub fn list_asset(
        env: Env,
        owner: Address,
        asset_contract: Address,
        token_id: u64,
        payment_token: Address,
        fee_per_period: i128,
        period_duration: u64,
        max_periods: u32,
    ) {
        owner.require_auth();

        assert!(fee_per_period >= 0, "fee cannot be negative");
        assert!(period_duration > 0, "period duration must be > 0");
        assert!(max_periods > 0 && max_periods <= 365, "max_periods must be 1-365");

        // Prevent duplicate listings
        assert!(
            !env.storage()
                .persistent()
                .has(&DataKey::Listing(asset_contract.clone(), token_id)),
            "asset already listed"
        );

        let listing = Listing {
            asset_contract: asset_contract.clone(),
            token_id,
            owner,
            payment_token,
            fee_per_period,
            period_duration,
            max_periods,
            status: ListingStatus::Available,
            created_at: env.ledger().timestamp(),
        };

        env.storage()
            .persistent()
            .set(&DataKey::Listing(asset_contract.clone(), token_id), &listing);

        // Register in index
        let mut index: Vec<ListingKey> = env
            .storage()
            .instance()
            .get(&DataKey::ListingIndex)
            .unwrap_or(Vec::new(&env));
        index.push_back(ListingKey {
            asset_contract,
            token_id,
        });
        env.storage()
            .instance()
            .set(&DataKey::ListingIndex, &index);
    }

    /// Update the fee or period duration on an available listing.
    pub fn update_listing(
        env: Env,
        owner: Address,
        asset_contract: Address,
        token_id: u64,
        new_fee_per_period: i128,
        new_period_duration: u64,
        new_max_periods: u32,
    ) {
        owner.require_auth();

        let mut listing: Listing = env
            .storage()
            .persistent()
            .get(&DataKey::Listing(asset_contract.clone(), token_id))
            .expect("listing not found");

        assert!(listing.owner == owner, "not the asset owner");
        assert!(
            listing.status == ListingStatus::Available,
            "can only update available listings"
        );
        assert!(new_fee_per_period >= 0, "fee cannot be negative");
        assert!(new_period_duration > 0, "period duration must be > 0");
        assert!(
            new_max_periods > 0 && new_max_periods <= 365,
            "max_periods must be 1-365"
        );

        listing.fee_per_period = new_fee_per_period;
        listing.period_duration = new_period_duration;
        listing.max_periods = new_max_periods;

        env.storage()
            .persistent()
            .set(&DataKey::Listing(asset_contract, token_id), &listing);
    }

    /// Delist an asset. Only possible if not currently rented or if rental expired.
    pub fn delist_asset(env: Env, owner: Address, asset_contract: Address, token_id: u64) {
        owner.require_auth();

        let mut listing: Listing = env
            .storage()
            .persistent()
            .get(&DataKey::Listing(asset_contract.clone(), token_id))
            .expect("listing not found");

        assert!(listing.owner == owner, "not the asset owner");
        assert!(
            listing.status != ListingStatus::Delisted,
            "already delisted"
        );

        // If currently rented, verify the rental has expired
        if listing.status == ListingStatus::Rented {
            let now = env.ledger().timestamp();
            let credential: RentalCredential = env
                .storage()
                .persistent()
                .get(&DataKey::Credential(asset_contract.clone(), token_id))
                .expect("credential missing for rented listing");

            assert!(
                now >= credential.expires_at || credential.terminated_early,
                "rental still active; wait for expiry or early termination"
            );

            // Auto-expire: mark credential as terminated
            // (credential naturally becomes invalid after expires_at anyway)
        }

        listing.status = ListingStatus::Delisted;
        env.storage()
            .persistent()
            .set(&DataKey::Listing(asset_contract, token_id), &listing);
    }

    // ── Renter: Rental Flow ──────────────────

    /// Rent an asset for a specified number of periods.
    ///
    /// Renter pays `fee_per_period × periods` from their payment token balance.
    /// Platform takes `PLATFORM_FEE_BPS` (2.5%) of the total fee.
    /// Owner receives the remainder.
    /// Renter receives a `RentalCredential` valid until `now + period_duration × periods`.
    pub fn rent(
        env: Env,
        renter: Address,
        asset_contract: Address,
        token_id: u64,
        periods: u32,
    ) {
        renter.require_auth();

        let mut listing: Listing = env
            .storage()
            .persistent()
            .get(&DataKey::Listing(asset_contract.clone(), token_id))
            .expect("listing not found");

        assert!(
            listing.status == ListingStatus::Available,
            "asset is not available for rent"
        );
        assert!(periods > 0, "must rent at least 1 period");
        assert!(
            periods <= listing.max_periods,
            "exceeds maximum rental periods"
        );

        let now = env.ledger().timestamp();
        let total_fee = listing.fee_per_period * periods as i128;
        let platform_fee = total_fee * PLATFORM_FEE_BPS / BPS_DENOM;
        let escrow_amount = total_fee - platform_fee;

        if total_fee > 0 {
            let payment_client = token::Client::new(&env, &listing.payment_token);

            // Pull full fee into contract escrow.
            payment_client.transfer(&renter, &env.current_contract_address(), &total_fee);

            // Forward platform fee immediately.
            if platform_fee > 0 {
                let fee_recipient: Address = env
                    .storage()
                    .instance()
                    .get(&DataKey::FeeRecipient)
                    .unwrap();
                payment_client.transfer(&env.current_contract_address(), &fee_recipient, &platform_fee);
            }
        }

        let expires_at = now + listing.period_duration * periods as u64;

        // Issue credential
        let credential = RentalCredential {
            asset_contract: asset_contract.clone(),
            token_id,
            renter: renter.clone(),
            owner: listing.owner.clone(),
            started_at: now,
            expires_at,
            periods,
            total_fee_paid: total_fee,
            escrow_amount,
            terminated_early: false,
        };

        env.storage()
            .persistent()
            .set(&DataKey::Credential(asset_contract.clone(), token_id), &credential);

        // Update listing status
        listing.status = ListingStatus::Rented;
        env.storage()
            .persistent()
            .set(&DataKey::Listing(asset_contract, token_id), &listing);
    }

    /// Early termination by the renter.
    ///
    /// Renter returns the asset before expiry. Owner receives a pro-rated
    /// share of the fee for time used. Renter receives a pro-rated refund
    /// for unused time. Listing becomes available again immediately.
    pub fn terminate_early(
        env: Env,
        renter: Address,
        asset_contract: Address,
        token_id: u64,
    ) {
        renter.require_auth();

        let listing: Listing = env
            .storage()
            .persistent()
            .get(&DataKey::Listing(asset_contract.clone(), token_id))
            .expect("listing not found");

        assert!(
            listing.status == ListingStatus::Rented,
            "asset is not currently rented"
        );

        let mut credential: RentalCredential = env
            .storage()
            .persistent()
            .get(&DataKey::Credential(asset_contract.clone(), token_id))
            .expect("credential not found");

        assert!(credential.renter == renter, "not the current renter");
        assert!(!credential.terminated_early, "already terminated");

        let now = env.ledger().timestamp();
        assert!(now < credential.expires_at, "rental has already expired");

        let total_duration = credential.expires_at - credential.started_at;
        let time_used = now - credential.started_at;
        let escrow = credential.escrow_amount;

        let owner_share = escrow * time_used as i128 / total_duration as i128;
        let renter_refund = escrow - owner_share;

        let payment_client = token::Client::new(&env, &listing.payment_token);
        if owner_share > 0 {
            payment_client.transfer(&env.current_contract_address(), &listing.owner, &owner_share);
        }
        if renter_refund > 0 {
            payment_client.transfer(&env.current_contract_address(), &renter, &renter_refund);
        }

        // Mark credential as terminated
        credential.terminated_early = true;
        env.storage()
            .persistent()
            .set(&DataKey::Credential(asset_contract.clone(), token_id), &credential);

        // Make listing available again
        let mut updated_listing = listing;
        updated_listing.status = ListingStatus::Available;
        env.storage()
            .persistent()
            .set(&DataKey::Listing(asset_contract, token_id), &updated_listing);
    }

    /// Called by the owner or anyone to expire a rental and reset listing to Available.
    /// Requires that the rental period has actually ended (now >= expires_at).
    /// This is a convenience function — the credential is already invalid after expiry,
    /// but calling this resets the listing status so new rentals can begin.
    pub fn expire_rental(env: Env, asset_contract: Address, token_id: u64) {
        let mut listing: Listing = env
            .storage()
            .persistent()
            .get(&DataKey::Listing(asset_contract.clone(), token_id))
            .expect("listing not found");

        assert!(
            listing.status == ListingStatus::Rented,
            "listing is not in rented state"
        );

        let credential: RentalCredential = env
            .storage()
            .persistent()
            .get(&DataKey::Credential(asset_contract.clone(), token_id))
            .expect("credential not found");

        let now = env.ledger().timestamp();
        assert!(
            now >= credential.expires_at || credential.terminated_early,
            "rental period has not ended yet"
        );

        // Release full escrow to owner on normal expiry.
        if credential.escrow_amount > 0 && !credential.terminated_early {
            let payment_client = token::Client::new(&env, &listing.payment_token);
            payment_client.transfer(
                &env.current_contract_address(),
                &listing.owner,
                &credential.escrow_amount,
            );
        }

        listing.status = ListingStatus::Available;
        env.storage()
            .persistent()
            .set(&DataKey::Listing(asset_contract, token_id), &listing);
    }

    // ── Validation ───────────────────────────

    /// Check whether a credential is currently valid for a given renter.
    /// Returns true only if: credential exists, renter matches, not terminated,
    /// and current time is before expires_at.
    pub fn is_credential_valid(
        env: Env,
        renter: Address,
        asset_contract: Address,
        token_id: u64,
    ) -> bool {
        let credential_opt: Option<RentalCredential> = env
            .storage()
            .persistent()
            .get(&DataKey::Credential(asset_contract, token_id));

        match credential_opt {
            None => false,
            Some(c) => {
                let now = env.ledger().timestamp();
                c.renter == renter && !c.terminated_early && now < c.expires_at
            }
        }
    }

    /// Check whether a credential has expired (past expires_at or terminated early).
    pub fn is_credential_expired(env: Env, asset_contract: Address, token_id: u64) -> bool {
        let credential_opt: Option<RentalCredential> = env
            .storage()
            .persistent()
            .get(&DataKey::Credential(asset_contract, token_id));

        match credential_opt {
            None => true,
            Some(c) => {
                let now = env.ledger().timestamp();
                now >= c.expires_at || c.terminated_early
            }
        }
    }

    // ── Queries ──────────────────────────────

    pub fn get_listing(env: Env, asset_contract: Address, token_id: u64) -> Listing {
        env.storage()
            .persistent()
            .get(&DataKey::Listing(asset_contract, token_id))
            .expect("listing not found")
    }

    pub fn get_credential(env: Env, asset_contract: Address, token_id: u64) -> RentalCredential {
        env.storage()
            .persistent()
            .get(&DataKey::Credential(asset_contract, token_id))
            .expect("credential not found")
    }

    pub fn get_all_listings(env: Env) -> Vec<ListingKey> {
        env.storage()
            .instance()
            .get(&DataKey::ListingIndex)
            .unwrap_or(Vec::new(&env))
    }

    pub fn get_admin(env: Env) -> Address {
        env.storage().instance().get(&ADMIN_KEY).unwrap()
    }

    pub fn get_fee_recipient(env: Env) -> Address {
        env.storage()
            .instance()
            .get(&DataKey::FeeRecipient)
            .unwrap()
    }

    /// Calculate total fee for renting an asset for N periods.
    pub fn quote_rental(env: Env, asset_contract: Address, token_id: u64, periods: u32) -> i128 {
        let listing: Listing = env
            .storage()
            .persistent()
            .get(&DataKey::Listing(asset_contract, token_id))
            .expect("listing not found");

        listing.fee_per_period * periods as i128
    }

    /// Update the platform fee recipient (admin only).
    pub fn set_fee_recipient(env: Env, admin: Address, new_recipient: Address) {
        admin.require_auth();
        let stored_admin: Address = env.storage().instance().get(&ADMIN_KEY).unwrap();
        assert!(stored_admin == admin, "not admin");
        env.storage()
            .instance()
            .set(&DataKey::FeeRecipient, &new_recipient);
    }
}

#[cfg(test)]
mod test;
