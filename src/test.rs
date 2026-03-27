use super::*;
use soroban_sdk::{
    testutils::{Address as _, Ledger, LedgerInfo},
    token::{Client as TokenClient, StellarAssetClient},
    Address, Env,
};

// ─────────────────────────────────────────────
// Test Helpers
// ─────────────────────────────────────────────

struct TestEnv {
    env: Env,
    admin: Address,
    fee_recipient: Address,
    payment_token: Address,
    _token_admin: Address,
    client: RentalContractClient<'static>,
}

fn setup() -> TestEnv {
    let env = Env::default();
    env.mock_all_auths();

    let contract_id = env.register(RentalContract, ());
    let client = RentalContractClient::new(&env, &contract_id);

    let admin = Address::generate(&env);
    let fee_recipient = Address::generate(&env);

    let token_admin = Address::generate(&env);
    let token_contract = env.register_stellar_asset_contract_v2(token_admin.clone());
    let payment_token = token_contract.address();

    client.initialize(&admin, &fee_recipient);

    TestEnv {
        env,
        admin,
        fee_recipient,
        payment_token,
        _token_admin: token_admin,
        client,
    }
}

fn set_time(env: &Env, timestamp: u64) {
    env.ledger().set(LedgerInfo {
        timestamp,
        protocol_version: 22,
        sequence_number: env.ledger().sequence(),
        network_id: Default::default(),
        base_reserve: 10,
        min_temp_entry_ttl: 1000,
        min_persistent_entry_ttl: 1000,
        max_entry_ttl: 6_312_000,
    });
}

fn mint(t: &TestEnv, to: &Address, amount: i128) {
    let asset_client = StellarAssetClient::new(&t.env, &t.payment_token);
    asset_client.mint(to, &amount);
}

fn token_balance(env: &Env, token: &Address, addr: &Address) -> i128 {
    TokenClient::new(env, token).balance(addr)
}

/// Create a standard listing: fee=1000, period=86400 (1 day), max=30
fn standard_listing(t: &TestEnv, owner: &Address, asset_contract: &Address, token_id: u64) {
    t.client.list_asset(
        owner,
        asset_contract,
        &token_id,
        &t.payment_token,
        &1000,
        &86_400,
        &30,
    );
}

// ─────────────────────────────────────────────
// Initialization Tests
// ─────────────────────────────────────────────

#[test]
fn test_initialize_sets_admin_and_fee_recipient() {
    let t = setup();
    assert_eq!(t.client.get_admin(), t.admin);
    assert_eq!(t.client.get_fee_recipient(), t.fee_recipient);
}

#[test]
#[should_panic(expected = "already initialized")]
fn test_double_initialize_panics() {
    let t = setup();
    let other = Address::generate(&t.env);
    t.client.initialize(&other, &other);
}

// ─────────────────────────────────────────────
// Listing Tests
// ─────────────────────────────────────────────

#[test]
fn test_list_asset_creates_available_listing() {
    let t = setup();
    let owner = Address::generate(&t.env);
    let asset = Address::generate(&t.env);

    standard_listing(&t, &owner, &asset, 1);

    let listing = t.client.get_listing(&asset, &1);
    assert_eq!(listing.owner, owner);
    assert_eq!(listing.fee_per_period, 1000);
    assert_eq!(listing.period_duration, 86_400);
    assert_eq!(listing.max_periods, 30);
    assert_eq!(listing.status, ListingStatus::Available);
}

#[test]
fn test_list_asset_appears_in_index() {
    let t = setup();
    let owner = Address::generate(&t.env);
    let asset = Address::generate(&t.env);

    standard_listing(&t, &owner, &asset, 42);

    let all = t.client.get_all_listings();
    assert_eq!(all.len(), 1);
    let key = all.get(0).unwrap();
    assert_eq!(key.asset_contract, asset);
    assert_eq!(key.token_id, 42);
}

#[test]
fn test_multiple_assets_listed() {
    let t = setup();
    let owner = Address::generate(&t.env);
    let asset1 = Address::generate(&t.env);
    let asset2 = Address::generate(&t.env);

    standard_listing(&t, &owner, &asset1, 1);
    standard_listing(&t, &owner, &asset2, 2);

    let all = t.client.get_all_listings();
    assert_eq!(all.len(), 2);
}

#[test]
#[should_panic(expected = "asset already listed")]
fn test_duplicate_listing_panics() {
    let t = setup();
    let owner = Address::generate(&t.env);
    let asset = Address::generate(&t.env);

    standard_listing(&t, &owner, &asset, 1);
    standard_listing(&t, &owner, &asset, 1);
}

#[test]
#[should_panic(expected = "period duration must be > 0")]
fn test_zero_period_duration_panics() {
    let t = setup();
    let owner = Address::generate(&t.env);
    let asset = Address::generate(&t.env);

    t.client.list_asset(&owner, &asset, &1, &t.payment_token, &100, &0, &10);
}

#[test]
#[should_panic(expected = "max_periods must be 1-365")]
fn test_zero_max_periods_panics() {
    let t = setup();
    let owner = Address::generate(&t.env);
    let asset = Address::generate(&t.env);

    t.client.list_asset(&owner, &asset, &1, &t.payment_token, &100, &86_400, &0);
}

#[test]
fn test_update_listing() {
    let t = setup();
    let owner = Address::generate(&t.env);
    let asset = Address::generate(&t.env);

    standard_listing(&t, &owner, &asset, 1);
    t.client.update_listing(&owner, &asset, &1, &2000, &3600, &7);

    let listing = t.client.get_listing(&asset, &1);
    assert_eq!(listing.fee_per_period, 2000);
    assert_eq!(listing.period_duration, 3600);
    assert_eq!(listing.max_periods, 7);
}

#[test]
fn test_delist_available_asset() {
    let t = setup();
    let owner = Address::generate(&t.env);
    let asset = Address::generate(&t.env);

    standard_listing(&t, &owner, &asset, 1);
    t.client.delist_asset(&owner, &asset, &1);

    let listing = t.client.get_listing(&asset, &1);
    assert_eq!(listing.status, ListingStatus::Delisted);
}

#[test]
#[should_panic(expected = "already delisted")]
fn test_double_delist_panics() {
    let t = setup();
    let owner = Address::generate(&t.env);
    let asset = Address::generate(&t.env);

    standard_listing(&t, &owner, &asset, 1);
    t.client.delist_asset(&owner, &asset, &1);
    t.client.delist_asset(&owner, &asset, &1);
}

// ─────────────────────────────────────────────
// Rental Tests
// ─────────────────────────────────────────────

#[test]
fn test_rent_issues_credential() {
    let t = setup();
    let owner = Address::generate(&t.env);
    let renter = Address::generate(&t.env);
    let asset = Address::generate(&t.env);

    set_time(&t.env, 1_000_000);
    standard_listing(&t, &owner, &asset, 1);
    mint(&t, &renter, 5000);

    t.client.rent(&renter, &asset, &1, &3); // 3 periods

    let credential = t.client.get_credential(&asset, &1);
    assert_eq!(credential.renter, renter);
    assert_eq!(credential.periods, 3);
    assert_eq!(credential.started_at, 1_000_000);
    assert_eq!(credential.expires_at, 1_000_000 + 86_400 * 3);
    assert!(!credential.terminated_early);
}

#[test]
fn test_rent_transfers_fee_to_owner_and_platform() {
    let t = setup();
    let owner = Address::generate(&t.env);
    let renter = Address::generate(&t.env);
    let asset = Address::generate(&t.env);

    set_time(&t.env, 0);
    standard_listing(&t, &owner, &asset, 1);

    // Total fee = 1000 * 2 = 2000
    // Platform fee = 2000 * 250 / 10000 = 50  (paid immediately at rent)
    // Escrow (owner proceeds) = 1950           (held in contract until expire_rental)
    mint(&t, &renter, 2000);

    t.client.rent(&renter, &asset, &1, &2);

    // After rent: platform got their cut; owner proceeds held in escrow
    assert_eq!(token_balance(&t.env, &t.payment_token, &renter), 0);
    assert_eq!(token_balance(&t.env, &t.payment_token, &owner), 0);
    assert_eq!(token_balance(&t.env, &t.payment_token, &t.fee_recipient), 50);

    // After expire_rental: owner receives escrow in full
    set_time(&t.env, 86_400 * 2 + 1);
    t.client.expire_rental(&asset, &1);
    assert_eq!(token_balance(&t.env, &t.payment_token, &owner), 1950);
}

#[test]
fn test_rent_marks_listing_as_rented() {
    let t = setup();
    let owner = Address::generate(&t.env);
    let renter = Address::generate(&t.env);
    let asset = Address::generate(&t.env);

    set_time(&t.env, 0);
    standard_listing(&t, &owner, &asset, 1);
    mint(&t, &renter, 1000);

    t.client.rent(&renter, &asset, &1, &1);

    let listing = t.client.get_listing(&asset, &1);
    assert_eq!(listing.status, ListingStatus::Rented);
}

#[test]
#[should_panic(expected = "asset is not available for rent")]
fn test_rent_already_rented_panics() {
    let t = setup();
    let owner = Address::generate(&t.env);
    let renter1 = Address::generate(&t.env);
    let renter2 = Address::generate(&t.env);
    let asset = Address::generate(&t.env);

    set_time(&t.env, 0);
    standard_listing(&t, &owner, &asset, 1);
    mint(&t, &renter1, 1000);
    mint(&t, &renter2, 1000);

    t.client.rent(&renter1, &asset, &1, &1);
    t.client.rent(&renter2, &asset, &1, &1); // should panic
}

#[test]
#[should_panic(expected = "exceeds maximum rental periods")]
fn test_rent_exceeds_max_periods_panics() {
    let t = setup();
    let owner = Address::generate(&t.env);
    let renter = Address::generate(&t.env);
    let asset = Address::generate(&t.env);

    set_time(&t.env, 0);
    standard_listing(&t, &owner, &asset, 1); // max 30 periods
    mint(&t, &renter, 100_000);

    t.client.rent(&renter, &asset, &1, &31); // exceeds max
}

#[test]
fn test_quote_rental_returns_correct_amount() {
    let t = setup();
    let owner = Address::generate(&t.env);
    let asset = Address::generate(&t.env);

    standard_listing(&t, &owner, &asset, 1); // fee=1000

    assert_eq!(t.client.quote_rental(&asset, &1, &1), 1000);
    assert_eq!(t.client.quote_rental(&asset, &1, &7), 7000);
    assert_eq!(t.client.quote_rental(&asset, &1, &30), 30_000);
}

// ─────────────────────────────────────────────
// Credential Validity Tests
// ─────────────────────────────────────────────

#[test]
fn test_credential_valid_during_rental_period() {
    let t = setup();
    let owner = Address::generate(&t.env);
    let renter = Address::generate(&t.env);
    let asset = Address::generate(&t.env);

    set_time(&t.env, 1_000_000);
    standard_listing(&t, &owner, &asset, 1);
    mint(&t, &renter, 1000);

    t.client.rent(&renter, &asset, &1, &1);

    // Mid-period: credential should be valid
    set_time(&t.env, 1_000_000 + 43_200); // half a day
    assert!(t.client.is_credential_valid(&renter, &asset, &1));
    assert!(!t.client.is_credential_expired(&asset, &1));
}

#[test]
fn test_credential_invalid_after_expiry() {
    let t = setup();
    let owner = Address::generate(&t.env);
    let renter = Address::generate(&t.env);
    let asset = Address::generate(&t.env);

    set_time(&t.env, 1_000_000);
    standard_listing(&t, &owner, &asset, 1);
    mint(&t, &renter, 1000);

    t.client.rent(&renter, &asset, &1, &1); // 1 day

    // After expiry
    set_time(&t.env, 1_000_000 + 86_400 + 1);
    assert!(!t.client.is_credential_valid(&renter, &asset, &1));
    assert!(t.client.is_credential_expired(&asset, &1));
}

#[test]
fn test_credential_invalid_for_wrong_renter() {
    let t = setup();
    let owner = Address::generate(&t.env);
    let renter = Address::generate(&t.env);
    let impostor = Address::generate(&t.env);
    let asset = Address::generate(&t.env);

    set_time(&t.env, 1_000_000);
    standard_listing(&t, &owner, &asset, 1);
    mint(&t, &renter, 1000);

    t.client.rent(&renter, &asset, &1, &1);

    set_time(&t.env, 1_000_000 + 100);
    assert!(t.client.is_credential_valid(&renter, &asset, &1));
    assert!(!t.client.is_credential_valid(&impostor, &asset, &1)); // impostor invalid
}

#[test]
fn test_no_credential_returns_expired() {
    let t = setup();
    let asset = Address::generate(&t.env);

    // No credential ever created
    assert!(t.client.is_credential_expired(&asset, &999));
}

// ─────────────────────────────────────────────
// Expire Rental Tests
// ─────────────────────────────────────────────

#[test]
fn test_expire_rental_resets_listing_to_available() {
    let t = setup();
    let owner = Address::generate(&t.env);
    let renter = Address::generate(&t.env);
    let asset = Address::generate(&t.env);

    set_time(&t.env, 0);
    standard_listing(&t, &owner, &asset, 1);
    mint(&t, &renter, 1000);

    t.client.rent(&renter, &asset, &1, &1);

    // After expiry, anyone can call expire_rental
    set_time(&t.env, 86_400 + 1);
    t.client.expire_rental(&asset, &1);

    let listing = t.client.get_listing(&asset, &1);
    assert_eq!(listing.status, ListingStatus::Available);
}

#[test]
#[should_panic(expected = "rental period has not ended yet")]
fn test_expire_rental_before_expiry_panics() {
    let t = setup();
    let owner = Address::generate(&t.env);
    let renter = Address::generate(&t.env);
    let asset = Address::generate(&t.env);

    set_time(&t.env, 0);
    standard_listing(&t, &owner, &asset, 1);
    mint(&t, &renter, 1000);

    t.client.rent(&renter, &asset, &1, &1);

    // Half-way through — should panic
    set_time(&t.env, 43_200);
    t.client.expire_rental(&asset, &1);
}

#[test]
fn test_re_rent_after_expiry() {
    let t = setup();
    let owner = Address::generate(&t.env);
    let renter1 = Address::generate(&t.env);
    let renter2 = Address::generate(&t.env);
    let asset = Address::generate(&t.env);

    set_time(&t.env, 0);
    standard_listing(&t, &owner, &asset, 1);
    mint(&t, &renter1, 1000);
    mint(&t, &renter2, 1000);

    t.client.rent(&renter1, &asset, &1, &1);

    // Expire rental
    set_time(&t.env, 86_400 + 1);
    t.client.expire_rental(&asset, &1);

    // Second renter can now rent
    t.client.rent(&renter2, &asset, &1, &1);
    assert!(t.client.is_credential_valid(&renter2, &asset, &1));
}

// ─────────────────────────────────────────────
// Early Termination Tests
// ─────────────────────────────────────────────

#[test]
fn test_terminate_early_marks_credential_terminated() {
    let t = setup();
    let owner = Address::generate(&t.env);
    let renter = Address::generate(&t.env);
    let asset = Address::generate(&t.env);

    set_time(&t.env, 0);
    standard_listing(&t, &owner, &asset, 1);

    // Rent for 2 days = 2000 total fee
    // platform_fee = 50, owner_proceeds = 1950
    // Renter mints 2000 for the rental
    mint(&t, &renter, 2000);
    t.client.rent(&renter, &asset, &1, &2);

    // Terminate at half-way (1 day into 2-day rental)
    // Escrow = 1950 (owner proceeds held in contract)
    // Owner share = 1950 * (86400/172800) = 975
    // Renter refund = 1950 - 975 = 975 (from escrow, no owner auth needed)
    set_time(&t.env, 86_400);
    t.client.terminate_early(&renter, &asset, &1);

    let credential = t.client.get_credential(&asset, &1);
    assert!(credential.terminated_early);
}

#[test]
fn test_terminate_early_resets_listing_to_available() {
    let t = setup();
    let owner = Address::generate(&t.env);
    let renter = Address::generate(&t.env);
    let asset = Address::generate(&t.env);

    set_time(&t.env, 0);
    standard_listing(&t, &owner, &asset, 1);
    mint(&t, &renter, 1000);

    t.client.rent(&renter, &asset, &1, &1);

    set_time(&t.env, 3600); // 1 hour in
    t.client.terminate_early(&renter, &asset, &1);

    let listing = t.client.get_listing(&asset, &1);
    assert_eq!(listing.status, ListingStatus::Available);
}

#[test]
fn test_credential_invalid_after_early_termination() {
    let t = setup();
    let owner = Address::generate(&t.env);
    let renter = Address::generate(&t.env);
    let asset = Address::generate(&t.env);

    set_time(&t.env, 0);
    standard_listing(&t, &owner, &asset, 1);
    mint(&t, &renter, 1000);

    t.client.rent(&renter, &asset, &1, &1);

    set_time(&t.env, 3600);
    t.client.terminate_early(&renter, &asset, &1);

    // Credential should now be invalid even though time hasn't expired
    assert!(!t.client.is_credential_valid(&renter, &asset, &1));
    assert!(t.client.is_credential_expired(&asset, &1));
}

#[test]
#[should_panic(expected = "not the current renter")]
fn test_terminate_early_wrong_renter_panics() {
    let t = setup();
    let owner = Address::generate(&t.env);
    let renter = Address::generate(&t.env);
    let impostor = Address::generate(&t.env);
    let asset = Address::generate(&t.env);

    set_time(&t.env, 0);
    standard_listing(&t, &owner, &asset, 1);
    mint(&t, &renter, 1000);

    t.client.rent(&renter, &asset, &1, &1);

    set_time(&t.env, 3600);
    t.client.terminate_early(&impostor, &asset, &1); // wrong renter
}

#[test]
#[should_panic(expected = "rental has already expired")]
fn test_terminate_early_after_expiry_panics() {
    let t = setup();
    let owner = Address::generate(&t.env);
    let renter = Address::generate(&t.env);
    let asset = Address::generate(&t.env);

    set_time(&t.env, 0);
    standard_listing(&t, &owner, &asset, 1);
    mint(&t, &renter, 1000);

    t.client.rent(&renter, &asset, &1, &1);

    // After expiry
    set_time(&t.env, 86_400 + 1);
    t.client.terminate_early(&renter, &asset, &1);
}

#[test]
#[should_panic(expected = "asset is not currently rented")]
fn test_double_terminate_panics() {
    let t = setup();
    let owner = Address::generate(&t.env);
    let renter = Address::generate(&t.env);
    let asset = Address::generate(&t.env);

    set_time(&t.env, 0);
    standard_listing(&t, &owner, &asset, 1);
    mint(&t, &renter, 1000);

    t.client.rent(&renter, &asset, &1, &1);

    set_time(&t.env, 3600);
    t.client.terminate_early(&renter, &asset, &1);
    t.client.terminate_early(&renter, &asset, &1); // should panic
}

// ─────────────────────────────────────────────
// Delist with Active Rental Tests
// ─────────────────────────────────────────────

#[test]
#[should_panic(expected = "rental still active")]
fn test_delist_during_active_rental_panics() {
    let t = setup();
    let owner = Address::generate(&t.env);
    let renter = Address::generate(&t.env);
    let asset = Address::generate(&t.env);

    set_time(&t.env, 0);
    standard_listing(&t, &owner, &asset, 1);
    mint(&t, &renter, 1000);

    t.client.rent(&renter, &asset, &1, &1);

    // Try to delist mid-rental
    set_time(&t.env, 3600);
    t.client.delist_asset(&owner, &asset, &1);
}

#[test]
fn test_delist_after_rental_expiry() {
    let t = setup();
    let owner = Address::generate(&t.env);
    let renter = Address::generate(&t.env);
    let asset = Address::generate(&t.env);

    set_time(&t.env, 0);
    standard_listing(&t, &owner, &asset, 1);
    mint(&t, &renter, 1000);

    t.client.rent(&renter, &asset, &1, &1);

    // After expiry owner can delist
    set_time(&t.env, 86_400 + 1);
    t.client.delist_asset(&owner, &asset, &1);

    let listing = t.client.get_listing(&asset, &1);
    assert_eq!(listing.status, ListingStatus::Delisted);
}

// ─────────────────────────────────────────────
// Fee Recipient Tests
// ─────────────────────────────────────────────

#[test]
fn test_set_fee_recipient() {
    let t = setup();
    let new_recipient = Address::generate(&t.env);
    t.client.set_fee_recipient(&t.admin, &new_recipient);
    assert_eq!(t.client.get_fee_recipient(), new_recipient);
}

#[test]
fn test_free_rental_no_token_transfer() {
    let t = setup();
    let owner = Address::generate(&t.env);
    let renter = Address::generate(&t.env);
    let asset = Address::generate(&t.env);

    set_time(&t.env, 0);
    // Free listing (fee = 0)
    t.client.list_asset(&owner, &asset, &1, &t.payment_token, &0, &86_400, &10);
    t.client.rent(&renter, &asset, &1, &1);

    // Credential is still issued
    let credential = t.client.get_credential(&asset, &1);
    assert_eq!(credential.renter, renter);
    assert_eq!(credential.total_fee_paid, 0);
    // No token balance changes needed since fee is 0
    assert_eq!(token_balance(&t.env, &t.payment_token, &renter), 0);
    assert_eq!(token_balance(&t.env, &t.payment_token, &owner), 0);
}
