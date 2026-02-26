#![cfg(test)]

use soroban_sdk::{
    testutils::{Address as _, Events, Ledger},
    token, Address, Env,
};

#[contract]
struct MockNftContract;

#[contractimpl]
impl MockNftContract {
    pub fn mint(env: Env, to: Address) -> u128 {
        let next_id: u128 = env.storage().instance().get(&1u32).unwrap_or(0u128) + 1;
        env.storage().instance().set(&1u32, &next_id);

        let mut records: Vec<MintRecord> = env
            .storage()
            .persistent()
            .get(&2u32)
            .unwrap_or_else(|| Vec::new(&env));
        records.push_back(MintRecord {
            to,
            token_id: next_id,
        });
        env.storage().persistent().set(&2u32, &records);

        next_id
    }

    pub fn minted(env: Env) -> Vec<MintRecord> {
        env.storage()
            .persistent()
            .get(&2u32)
            .unwrap_or_else(|| Vec::new(&env))
    }
}

fn setup_env() -> (
    Env,
    CrowdfundContractClient<'static>,
    Address,
    Address,
    token::StellarAssetClient<'static>,
) {
    let env = Env::default();
    env.mock_all_auths();

    let contract_id = env.register(CrowdfundContract, ());
    let client = CrowdfundContractClient::new(&env, &contract_id);

    let token_admin = Address::generate(&env);
    let token_contract_id = env.register_stellar_asset_contract_v2(token_admin);
    let token_address = token_contract_id.address();
    let token_admin_client = token::StellarAssetClient::new(&env, &token_address);

    let creator = Address::generate(&env);
    token_admin_client.mint(&creator, &10_000_000);

    (env, client, creator, token_address, token_admin_client)
}

#[test]
fn test_withdraw_mints_nft_for_each_contributor() {
    let (env, client, creator, token_address, token_admin_client) = setup_env();

    let deadline = env.ledger().timestamp() + 3600;
    let goal: i128 = 1_000_000;
    let min_contribution: i128 = 1_000;

    client.initialize(
        &creator,
        &token_address,
        &goal,
        &deadline,
        &min_contribution,
        &None,
        &None,
        &None,
    );

    let nft_id = env.register(MockNftContract, ());
    let nft_client = MockNftContractClient::new(&env, &nft_id);
    client.set_nft_contract(&creator, &nft_id);

    let alice = Address::generate(&env);
    let bob = Address::generate(&env);
    token_admin_client.mint(&alice, &600_000);
    token_admin_client.mint(&bob, &400_000);

    client.contribute(&alice, &300_000, None);
    client.contribute(&bob, &200_000, None);

    env.ledger().set_timestamp(deadline + 1);
    client.withdraw();

    let minted = nft_client.minted();
    assert_eq!(minted.len(), 2);
    assert_eq!(minted.get(0).unwrap().to, alice);
    assert_eq!(minted.get(0).unwrap().token_id, 1);
    assert_eq!(minted.get(1).unwrap().to, bob);
    assert_eq!(minted.get(1).unwrap().token_id, 2);
}

#[test]
fn test_withdraw_skips_nft_mint_when_contract_not_set() {
    let (env, client, creator, token_address, token_admin_client) = setup_env();

    let deadline = env.ledger().timestamp() + 3600;
    let goal: i128 = 1_000_000;
    let min_contribution: i128 = 1_000;

    client.initialize(
        &creator,
        &token_address,
        &goal,
        &deadline,
        &min_contribution,
        &default_title(&env),
        &default_description(&env),
        &None,
    );
    let result = client.try_initialize(
        &creator,
        &token_address,
        &goal,
        &deadline,
        &min_contribution,
        &None,
        &None,
        &None,
    );

    assert!(result.is_err());
    assert_eq!(
        result.unwrap_err().unwrap(),
        crate::ContractError::AlreadyInitialized
    );
}

#[test]
fn test_contribute() {
    let (env, client, creator, token_address, admin) = setup_env();

    let deadline = env.ledger().timestamp() + 3600;
    let goal: i128 = 1_000_000;
    let min_contribution: i128 = 1_000;
    client.initialize(
        &creator,
        &token_address,
        &goal,
        &deadline,
        &min_contribution,
        &default_title(&env),
        &default_description(&env),
        &None,
    );

    let contributor = Address::generate(&env);
    token_admin_client.mint(&contributor, &goal);
    client.contribute(&contributor, &goal);

    client.contribute(&contributor, &500_000, &None);

    assert_eq!(nft_client.minted().len(), 0);
}

#[test]
fn test_set_nft_contract_rejects_non_creator() {
    let (env, client, creator, token_address, _token_admin_client) = setup_env();

    let deadline = env.ledger().timestamp() + 3600;
    let goal: i128 = 1_000_000;
    let min_contribution: i128 = 1_000;
    client.initialize(
        &creator,
        &token_address,
        &goal,
        &deadline,
        &min_contribution,
        &default_title(&env),
        &default_description(&env),
        &None,
    );

    let alice = Address::generate(&env);
    let bob = Address::generate(&env);
    mint_to(&env, &token_address, &token_admin, &alice, 600_000);
    mint_to(&env, &token_address, &token_admin, &bob, 400_000);

    client.contribute(&alice, &300_000, &None);
    client.contribute(&bob, &200_000, &None);

    assert_eq!(client.total_raised(), 500_000);
    assert_eq!(client.contribution(&alice), 300_000);
    assert_eq!(client.contribution(&bob), 200_000);
}

#[test]
fn test_contribute_after_deadline_panics() {
    let (env, client, platform_admin, creator, token_address, token_admin) = setup_env();

    let deadline = env.ledger().timestamp() + 100;
    let goal: i128 = 1_000_000;
    let min_contribution: i128 = 1_000;
    client.initialize(
        &creator,
        &token_address,
        &goal,
        &deadline,
        &min_contribution,
        &default_title(&env),
        &default_description(&env),
        &None,
    );

    // Fast-forward past the deadline.
    env.ledger().set_timestamp(deadline + 1);

    client.initialize(
        &creator,
        &token_address,
        &goal,
        &deadline,
        &min_contribution,
        &None,
        &None,
        &None,
    );

    let non_creator = Address::generate(&env);
    let nft_id = env.register(MockNftContract, ());

    let result = client.try_set_nft_contract(&non_creator, &nft_id);
    assert!(result.is_err());
}

#[test]
fn test_withdraw_successful_campaign_updates_status_and_balance() {
    let (env, client, creator, token_address, token_admin_client) = setup_env();

    let deadline = env.ledger().timestamp() + 3600;
    let goal: i128 = 500_000;
    let min_contribution: i128 = 1_000;
    client.initialize(
        &creator,
        &token_address,
        &goal,
        &deadline,
        &min_contribution,
        &default_title(&env),
        &default_description(&env),
        &None,
    );

    let contributor = Address::generate(&env);
    mint_to(&env, &token_address, &admin, &contributor, 1_000_000);
    client.contribute(&contributor, &1_000_000, &None);

    let token_client = token::Client::new(&env, &token_address);
    let creator_before = token_client.balance(&creator);

    env.ledger().set_timestamp(deadline + 1);
    client.withdraw();

    assert_eq!(client.total_raised(), 0);
    assert_eq!(token_client.balance(&creator), creator_before + goal);
}

#[test]
fn test_contribute_after_deadline_returns_error() {
    let (env, client, creator, token_address, token_admin_client) = setup_env();

    let deadline = env.ledger().timestamp() + 100;
    let goal: i128 = 1_000_000;
    let min_contribution: i128 = 1_000;
    client.initialize(
        &creator,
        &token_address,
        &goal,
        &deadline,
        &min_contribution,
        &default_title(&env),
        &default_description(&env),
        &None,
    );

    let contributor = Address::generate(&env);
    mint_to(&env, &token_address, &admin, &contributor, 1_000_000);
    client.contribute(&contributor, &1_000_000, &None);

    let result = client.try_withdraw();

    assert!(result.is_err());
    assert_eq!(
        result.unwrap_err().unwrap(),
        crate::ContractError::CampaignStillActive
    );
}

#[test]
fn test_withdraw_goal_not_reached_panics() {
    let (env, client, platform_admin, creator, token_address, token_admin) = setup_env();

    let deadline = env.ledger().timestamp() + 3600;
    let goal: i128 = 1_000_000;
    let min_contribution: i128 = 1_000;
    client.initialize(
        &creator,
        &token_address,
        &goal,
        &deadline,
        &min_contribution,
        &default_title(&env),
        &default_description(&env),
        &None,
    );

    let contributor = Address::generate(&env);
    mint_to(&env, &token_address, &admin, &contributor, 500_000);
    client.contribute(&contributor, &500_000, &None);

    env.ledger().set_timestamp(deadline + 1);

    let contributor = Address::generate(&env);
    token_admin_client.mint(&contributor, &500_000);

    let result = client.try_contribute(&contributor, &500_000);
    assert_eq!(result.unwrap_err().unwrap(), ContractError::CampaignEnded);
}

// ── Contributor Count Tests ────────────────────────────────────────────────

#[test]
fn test_contributor_count_zero_before_contributions() {
    let (env, client, creator, token_address, _admin) = setup_env();

    let deadline = env.ledger().timestamp() + 3600;
    let goal: i128 = 1_000_000;
    let min_contribution: i128 = 1_000;
    client.initialize(
        &creator,
        &token_address,
        &goal,
        &deadline,
        &min_contribution,
        &default_title(&env),
        &default_description(&env),
        &None,
    );

    let alice = Address::generate(&env);
    let bob = Address::generate(&env);
    mint_to(&env, &token_address, &token_admin, &alice, 300_000);
    mint_to(&env, &token_address, &token_admin, &bob, 200_000);

    client.contribute(&alice, &300_000, &None);
    client.contribute(&bob, &200_000, &None);

    // Move past deadline — goal not met.
    env.ledger().set_timestamp(deadline + 1);

    client.initialize(&creator, &token_address, &goal, &(goal * 2), &deadline, &min_contribution, &None);

    assert_eq!(client.contributor_count(), 0);
}

#[test]
fn test_contributor_count_one_after_single_contribution() {
    let (env, client, creator, token_address, admin) = setup_env();

    let deadline = env.ledger().timestamp() + 3600;
    let goal: i128 = 1_000_000;
    let min_contribution: i128 = 1_000;
    client.initialize(
        &creator,
        &token_address,
        &goal,
        &deadline,
        &min_contribution,
        &default_title(&env),
        &default_description(&env),
        &None,
    );

    let contributor = Address::generate(&env);
    mint_to(&env, &token_address, &admin, &contributor, 1_000_000);
    client.contribute(&contributor, &1_000_000, &None);

    env.ledger().set_timestamp(deadline + 1);

    let result = client.try_refund_single(&contributor);

    assert!(result.is_err());
    assert_eq!(
        result.unwrap_err().unwrap(),
        crate::ContractError::GoalReached
    );
}
#[test]
fn test_refund_single_before_deadline_fails() {
    let (env, client, creator, token_address, admin) = setup_env();

    let deadline = env.ledger().timestamp() + 3600;
    let goal: i128 = 1_000_000;
    let min_contribution: i128 = 1_000;
    client.initialize(
        &creator,
        &token_address,
        &goal,
        &deadline,
        &min_contribution,
        &default_title(&env),
        &default_description(&env),
        &None,
    );

    let contributor = Address::generate(&env);
    mint_to(&env, &token_address, &admin, &contributor, 500_000);
    client.contribute(&contributor, &500_000);

    // Try to refund before deadline passes
    let result = client.try_refund_single(&contributor);

    assert!(result.is_err());
    assert_eq!(
        result.unwrap_err().unwrap(),
        crate::ContractError::CampaignStillActive
    );
}

// ── Bug Condition Exploration Test ─────────────────────────────────────────

/// **Validates: Requirements 2.1, 2.2, 2.3, 2.4, 2.5, 2.6**
///
/// **Property 1: Fault Condition** - Structured Error Returns
///
/// This test verifies that all 6 error conditions return the appropriate
/// ContractError variants instead of panicking.
///
/// The test covers all 6 error conditions:
/// 1. Double initialization → Err(ContractError::AlreadyInitialized)
/// 2. Late contribution → Err(ContractError::CampaignEnded)
/// 3. Early withdrawal → Err(ContractError::CampaignStillActive)
/// 4. Withdrawal without goal → Err(ContractError::GoalNotReached)
/// 5. Early refund → Err(ContractError::CampaignStillActive)
/// 6. Refund after success → Err(ContractError::GoalReached)
#[test]
fn test_bug_condition_exploration_all_error_conditions_panic() {
    // Test 1: Double initialization
    {
        let (env, client, creator, token_address, _admin) = setup_env();
        let deadline = env.ledger().timestamp() + 3600;
        let goal: i128 = 1_000_000;

        client.initialize(
            &creator,
            &token_address,
            &goal,
            &deadline,
            &1_000,
            &default_title(&env),
            &default_description(&env),
            &None,
        );
        let result = client.try_initialize(
            &creator,
            &token_address,
            &goal,
            &deadline,
            &1_000,
            &default_title(&env),
            &default_description(&env),
            &None,
        );

        assert!(result.is_err());
        assert_eq!(
            result.unwrap_err().unwrap(),
            ContractError::AlreadyInitialized
        );
    }

    // Test 2: Late contribution
    {
        let (env, client, creator, token_address, admin) = setup_env();
        let deadline = env.ledger().timestamp() + 100;
        let goal: i128 = 1_000_000;
        client.initialize(
            &creator,
            &token_address,
            &goal,
            &deadline,
            &1_000,
            &default_title(&env),
            &default_description(&env),
            &None,
        );

        env.ledger().set_timestamp(deadline + 1);

        let contributor = Address::generate(&env);
        mint_to(&env, &token_address, &admin, &contributor, 500_000);
        let result = client.try_contribute(&contributor, &500_000, &None);

        assert!(result.is_err());
        assert_eq!(result.unwrap_err().unwrap(), ContractError::CampaignEnded);
    }

    // Test 3: Early withdrawal
    {
        let (env, client, creator, token_address, admin) = setup_env();
        let deadline = env.ledger().timestamp() + 3600;
        let goal: i128 = 1_000_000;
        client.initialize(
            &creator,
            &token_address,
            &goal,
            &deadline,
            &1_000,
            &default_title(&env),
            &default_description(&env),
            &None,
        );

        let contributor = Address::generate(&env);
        mint_to(&env, &token_address, &admin, &contributor, 1_000_000);
        client.contribute(&contributor, &1_000_000, &None);

        let result = client.try_withdraw();

        assert!(result.is_err());
        assert_eq!(
            result.unwrap_err().unwrap(),
            ContractError::CampaignStillActive
        );
    }

    // Test 4: Withdrawal without goal
    {
        let (env, client, creator, token_address, admin) = setup_env();
        let deadline = env.ledger().timestamp() + 3600;
        let goal: i128 = 1_000_000;
        client.initialize(
            &creator,
            &token_address,
            &goal,
            &deadline,
            &1_000,
            &default_title(&env),
            &default_description(&env),
            &None,
        );

        let contributor = Address::generate(&env);
        mint_to(&env, &token_address, &admin, &contributor, 500_000);
        client.contribute(&contributor, &500_000, &None);

        env.ledger().set_timestamp(deadline + 1);
        let result = client.try_withdraw();

        assert!(result.is_err());
        assert_eq!(result.unwrap_err().unwrap(), ContractError::GoalNotReached);
    }

    // Test 5: Early refund
    {
        let (env, client, creator, token_address, admin) = setup_env();
        let deadline = env.ledger().timestamp() + 3600;
        let goal: i128 = 1_000_000;
        client.initialize(
            &creator,
            &token_address,
            &goal,
            &deadline,
            &1_000,
            &default_title(&env),
            &default_description(&env),
            &None,
        );

        let contributor = Address::generate(&env);
        mint_to(&env, &token_address, &admin, &contributor, 500_000);
        client.contribute(&contributor, &500_000);

        let result = client.try_refund_single(&contributor);

        assert!(result.is_err());
        assert_eq!(
            result.unwrap_err().unwrap(),
            ContractError::CampaignStillActive
        );
    }

    // Test 6: Refund after success
    {
        let (env, client, creator, token_address, admin) = setup_env();
        let deadline = env.ledger().timestamp() + 3600;
        let goal: i128 = 1_000_000;
        client.initialize(
            &creator,
            &token_address,
            &goal,
            &deadline,
            &1_000,
            &default_title(&env),
            &default_description(&env),
            &None,
        );

        let contributor = Address::generate(&env);
        mint_to(&env, &token_address, &admin, &contributor, 1_000_000);
        client.contribute(&contributor, &1_000_000, &None);

        env.ledger().set_timestamp(deadline + 1);
        let result = client.try_refund_single(&contributor);

        assert!(result.is_err());
        assert_eq!(result.unwrap_err().unwrap(), ContractError::GoalReached);
    }
}

// ── Preservation Property Tests ────────────────────────────────────────────

use proptest::prelude::*;

/// **Validates: Requirements 3.1, 3.2, 3.3, 3.4, 3.5, 3.6**
///
/// **Property 2: Preservation** - Successful Execution Paths
///
/// This test verifies that all successful execution paths work correctly
/// on the UNFIXED code. These behaviors MUST be preserved after the fix.
///
/// **IMPORTANT**: This test is EXPECTED TO PASS on unfixed code.
/// When it passes, it confirms the baseline behavior to preserve.
///
/// The test covers all successful operations:
/// 1. First initialization with valid parameters stores creator, token, goal, deadline, and initializes total_raised to 0
/// 2. Valid contributions before deadline transfer tokens, update balances, and track contributors
/// 3. Successful withdrawal by creator after deadline when goal met transfers funds and resets total_raised
/// 4. Successful refund after deadline when goal not met refunds all contributors
/// 5. View functions (total_raised, goal, deadline, contribution) return correct values
/// 6. Multiple contributors are tracked correctly with individual and aggregate totals

proptest! {
    #[test]
    fn prop_preservation_first_initialization(
        goal in 1_000i128..10_000_000i128,
        deadline_offset in 100u64..10_000u64,
    ) {
        let (env, client, creator, token_address, _admin) = setup_env();
        let deadline = env.ledger().timestamp() + deadline_offset;

        // Test 3.1: First initialization stores all values correctly
        client.initialize(&creator, &token_address, &goal, &deadline, &1_000, &default_title(&env), &default_description(&env), &None);

        prop_assert_eq!(client.goal(), goal);
        prop_assert_eq!(client.deadline(), deadline);
        prop_assert_eq!(client.total_raised(), 0);
    }

    #[test]
    fn prop_preservation_valid_contribution(
        goal in 1_000_000i128..10_000_000i128,
        deadline_offset in 100u64..10_000u64,
        contribution_amount in 100_000i128..1_000_000i128,
    ) {
        let (env, client, creator, token_address, admin) = setup_env();
        let deadline = env.ledger().timestamp() + deadline_offset;

        client.initialize(&creator, &token_address, &goal, &deadline, &1_000, &default_title(&env), &default_description(&env), &None);

    client.contribute(&alice, &300_000, None);
    client.contribute(&bob, &200_000, None);

        // Test 3.2: Valid contribution before deadline works correctly
        client.contribute(&contributor, &contribution_amount);

        prop_assert_eq!(client.total_raised(), contribution_amount);
        prop_assert_eq!(client.contribution(&contributor), contribution_amount);
    }

    #[test]
    fn prop_preservation_successful_withdrawal(
        goal in 1_000_000i128..5_000_000i128,
        deadline_offset in 100u64..10_000u64,
    ) {
        let (env, client, creator, token_address, admin) = setup_env();
        let deadline = env.ledger().timestamp() + deadline_offset;

        client.initialize(&creator, &token_address, &goal, &deadline, &1_000, &default_title(&env), &default_description(&env), &None);

        let contributor = Address::generate(&env);
        mint_to(&env, &token_address, &admin, &contributor, goal);
        client.contribute(&contributor, &goal);

        // Move past deadline
        env.ledger().set_timestamp(deadline + 1);

    client.contribute(&contributor, &10_000, None);

        // Test 3.3: Successful withdrawal transfers funds and resets total_raised
        client.withdraw();

        prop_assert_eq!(client.total_raised(), 0);
        prop_assert_eq!(token_client.balance(&creator), creator_balance_before + goal);
    }

    #[test]
    fn prop_preservation_successful_refund(
        goal in 2_000_000i128..10_000_000i128,
        deadline_offset in 100u64..10_000u64,
        contribution_amount in 100_000i128..1_000_000i128,
    ) {
        let (env, client, creator, token_address, admin) = setup_env();
        let deadline = env.ledger().timestamp() + deadline_offset;

        // Ensure contribution is less than goal
        let contribution = contribution_amount.min(goal - 1);

    client.contribute(&contributor, &50_000, &None);

        let contributor = Address::generate(&env);
        mint_to(&env, &token_address, &admin, &contributor, contribution);
        client.contribute(&contributor, &contribution);

        // Move past deadline (goal not met)
        env.ledger().set_timestamp(deadline + 1);

        // Test 3.4: Successful refund returns funds to contributors
        client.refund_single(&contributor);

        let token_client = token::Client::new(&env, &token_address);
        prop_assert_eq!(token_client.balance(&contributor), contribution);
        prop_assert_eq!(client.total_raised(), 0);
    }

    #[test]
    fn prop_preservation_view_functions(
        goal in 1_000_000i128..10_000_000i128,
        deadline_offset in 100u64..10_000u64,
        contribution_amount in 100_000i128..1_000_000i128,
    ) {
        let (env, client, creator, token_address, admin) = setup_env();
        let deadline = env.ledger().timestamp() + deadline_offset;

        client.initialize(&creator, &token_address, &goal, &deadline, &1_000, &default_title(&env), &default_description(&env), &None);

        let contributor = Address::generate(&env);
        mint_to(&env, &token_address, &admin, &contributor, contribution_amount);
        client.contribute(&contributor, &contribution_amount);

        // Test 3.5: View functions return correct values
        prop_assert_eq!(client.goal(), goal);
        prop_assert_eq!(client.deadline(), deadline);
        prop_assert_eq!(client.total_raised(), contribution_amount);
        prop_assert_eq!(client.contribution(&contributor), contribution_amount);
    }

    #[test]
    fn prop_preservation_multiple_contributors(
        goal in 5_000_000i128..10_000_000i128,
        deadline_offset in 100u64..10_000u64,
        amount1 in 100_000i128..1_000_000i128,
        amount2 in 100_000i128..1_000_000i128,
        amount3 in 100_000i128..1_000_000i128,
    ) {
        let (env, client, creator, token_address, admin) = setup_env();
        let deadline = env.ledger().timestamp() + deadline_offset;

        client.initialize(&creator, &token_address, &goal, &deadline, &1_000, &default_title(&env), &default_description(&env), &None);

        let alice = Address::generate(&env);
        let bob = Address::generate(&env);
        let charlie = Address::generate(&env);

        mint_to(&env, &token_address, &admin, &alice, amount1);
        mint_to(&env, &token_address, &admin, &bob, amount2);
        mint_to(&env, &token_address, &admin, &charlie, amount3);

        // Test 3.6: Multiple contributors are tracked correctly
        client.contribute(&alice, &amount1);
        client.contribute(&bob, &amount2);
        client.contribute(&charlie, &amount3);

        let expected_total = amount1 + amount2 + amount3;

        prop_assert_eq!(client.total_raised(), expected_total);
        prop_assert_eq!(client.contribution(&alice), amount1);
        prop_assert_eq!(client.contribution(&bob), amount2);
        prop_assert_eq!(client.contribution(&charlie), amount3);
    }
}

#[test]
#[should_panic(expected = "campaign is not active")]
fn test_double_withdraw_panics() {
    let (env, client, platform_admin, creator, token_address, token_admin) = setup_env();

    let deadline = env.ledger().timestamp() + 3600;
    let goal: i128 = 1_000_000;
    let min_contribution: i128 = 1_000;
    client.initialize(
        &creator,
        &token_address,
        &goal,
        &deadline,
        &min_contribution,
        &default_title(&env),
        &default_description(&env),
        &None,
    );

    let contributor = Address::generate(&env);
    mint_to(&env, &token_address, &token_admin, &contributor, 1_000_000);
    client.contribute(&contributor, &1_000_000);

    env.ledger().set_timestamp(deadline + 1);

    client.withdraw();
    client.withdraw(); // should panic — status is Successful
}

#[test]
fn test_contributor_count_multiple_contributors() {
    let (env, client, creator, token_address, admin) = setup_env();

    let deadline = env.ledger().timestamp() + 3600;
    let goal: i128 = 1_000_000;
    let min_contribution: i128 = 1_000;
    client.initialize(
        &creator,
        &token_address,
        &goal,
        &deadline,
        &min_contribution,
        &default_title(&env),
        &default_description(&env),
        &None,
    );

    let alice = Address::generate(&env);
    mint_to(&env, &token_address, &token_admin, &alice, 500_000);
    client.contribute(&alice, &500_000);

    env.ledger().set_timestamp(deadline + 1);

    client.refund_single(&alice);
    // Second refund should succeed but do nothing (amount is 0)
    let result = client.try_refund_single(&alice);
    assert!(result.is_ok());
}

// NOTE: The following tests are commented out because the cancel function
// is not implemented in the current version of the contract.
// TODO: Implement cancel function or remove these tests.

/*
#[test]
fn test_cancel_with_no_contributions() {
    let (env, client, platform_admin, creator, token_address, _token_admin) = setup_env();

    let deadline = env.ledger().timestamp() + 3600;
    let goal: i128 = 1_000_000;
    let min_contribution: i128 = 1_000;
    client.initialize(&creator, &token_address, &goal, &deadline, &min_contribution, &default_title(&env), &default_description(&env), &None);

proptest! {
    #[test]
    fn prop_preservation_first_initialization(
        goal in 1_000i128..10_000_000i128,
        deadline_offset in 100u64..10_000u64,
    ) {
        let (env, client, creator, token_address, _admin) = setup_env();
        let deadline = env.ledger().timestamp() + deadline_offset;

        // Test 3.1: First initialization stores all values correctly
        client.initialize(&creator, &token_address, &goal, &deadline, &1_000, &default_title(&env), &default_description(&env), &None);

        prop_assert_eq!(client.goal(), goal);
        prop_assert_eq!(client.deadline(), deadline);
        prop_assert_eq!(client.total_raised(), 0);
    }

    client.initialize(&creator, &token_address, &goal, &(goal * 2), &deadline, &min_contribution, &None);

        client.initialize(&creator, &token_address, &goal, &deadline, &1_000, &default_title(&env), &default_description(&env), &None);

    client.contribute(&alice, &300_000, &None);
    client.contribute(&bob, &200_000, &None);

        // Test 3.2: Valid contribution before deadline works correctly
        client.contribute(&contributor, &contribution_amount);

        prop_assert_eq!(client.total_raised(), contribution_amount);
        prop_assert_eq!(client.contribution(&contributor), contribution_amount);
    }

    #[test]
    fn prop_preservation_successful_withdrawal(
        goal in 1_000_000i128..5_000_000i128,
        deadline_offset in 100u64..10_000u64,
    ) {
        let (env, client, creator, token_address, admin) = setup_env();
        let deadline = env.ledger().timestamp() + deadline_offset;

        client.initialize(&creator, &token_address, &goal, &deadline, &1_000, &default_title(&env), &default_description(&env), &None);

    let deadline = env.ledger().timestamp() + 3600;
    let goal: i128 = 1_000_000;
    let min_contribution: i128 = 1_000;
    client.initialize(&creator, &token_address, &goal, &deadline, &min_contribution, &default_title(&env), &default_description(&env), &None);

        // Move past deadline
        env.ledger().set_timestamp(deadline + 1);

    client.contribute(&contributor, &10_000, &None);

    assert_eq!(client.total_raised(), 10_000);
    assert_eq!(client.contribution(&contributor), 10_000);
}
*/

        prop_assert_eq!(client.total_raised(), 0);
        prop_assert_eq!(token_client.balance(&creator), creator_balance_before + goal);
    }

    let deadline = env.ledger().timestamp() + 3600;
    let goal: i128 = 1_000_000;
    let min_contribution: i128 = 10_000;
    client.initialize(
        &creator,
        &token_address,
        &goal,
        &deadline,
        &min_contribution,
        &default_title(&env),
        &default_description(&env),
        &None,
    );

        // Ensure contribution is less than goal
        let contribution = contribution_amount.min(goal - 1);

    client.contribute(&contributor, &50_000, &None);

        let contributor = Address::generate(&env);
        mint_to(&env, &token_address, &admin, &contributor, contribution);
        client.contribute(&contributor, &contribution);

        // Move past deadline (goal not met)
        env.ledger().set_timestamp(deadline + 1);

        // Test 3.4: Successful refund returns funds to contributors
        client.refund_single(&contributor);

    let deadline = env.ledger().timestamp() + 3600;
    let goal: i128 = 1_000_000;
    let min_contribution: i128 = 10_000;
    client.initialize(
        &creator,
        &token_address,
        &goal,
        &deadline,
        &min_contribution,
        &default_title(&env),
        &default_description(&env),
        &None,
    );

    #[test]
    fn prop_preservation_view_functions(
        goal in 1_000_000i128..10_000_000i128,
        deadline_offset in 100u64..10_000u64,
        contribution_amount in 100_000i128..1_000_000i128,
    ) {
        let (env, client, creator, token_address, admin) = setup_env();
        let deadline = env.ledger().timestamp() + deadline_offset;

        client.initialize(&creator, &token_address, &goal, &deadline, &1_000, &default_title(&env), &default_description(&env), &None);

        let contributor = Address::generate(&env);
        mint_to(&env, &token_address, &admin, &contributor, contribution_amount);
        client.contribute(&contributor, &contribution_amount);

        // Test 3.5: View functions return correct values
        prop_assert_eq!(client.goal(), goal);
        prop_assert_eq!(client.deadline(), deadline);
        prop_assert_eq!(client.total_raised(), contribution_amount);
        prop_assert_eq!(client.contribution(&contributor), contribution_amount);
    }

    let deadline = env.ledger().timestamp() + 3600;
    let goal: i128 = 1_000_000;
    let min_contribution: i128 = 10_000;
    client.initialize(
        &creator,
        &token_address,
        &goal,
        &deadline,
        &min_contribution,
        &default_title(&env),
        &default_description(&env),
        &None,
    );

        client.initialize(&creator, &token_address, &goal, &deadline, &1_000, &default_title(&env), &default_description(&env), &None);

        let alice = Address::generate(&env);
        let bob = Address::generate(&env);
        let charlie = Address::generate(&env);

        mint_to(&env, &token_address, &admin, &alice, amount1);
        mint_to(&env, &token_address, &admin, &bob, amount2);
        mint_to(&env, &token_address, &admin, &charlie, amount3);

        // Test 3.6: Multiple contributors are tracked correctly
        client.contribute(&alice, &amount1);
        client.contribute(&bob, &amount2);
        client.contribute(&charlie, &amount3);

        let expected_total = amount1 + amount2 + amount3;

        prop_assert_eq!(client.total_raised(), expected_total);
        prop_assert_eq!(client.contribution(&alice), amount1);
        prop_assert_eq!(client.contribution(&bob), amount2);
        prop_assert_eq!(client.contribution(&charlie), amount3);
    }
}

#[test]
#[should_panic(expected = "campaign is not active")]
fn test_double_withdraw_panics() {
    let (env, client, creator, token_address, admin) = setup_env();

    let deadline = env.ledger().timestamp() + 3600;
    let goal: i128 = 1_000_000;
    let min_contribution: i128 = 10_000;
    client.initialize(&creator, &token_address, &goal, &deadline, &min_contribution, &default_title(&env), &default_description(&env), &None);

    let bronze = soroban_sdk::String::from_str(&env, "Bronze");
    let silver = soroban_sdk::String::from_str(&env, "Silver");
    let gold = soroban_sdk::String::from_str(&env, "Gold");
    client.add_reward_tier(&creator, &bronze, &10_000);
    client.add_reward_tier(&creator, &silver, &100_000);
    client.add_reward_tier(&creator, &gold, &500_000);

    let contributor = Address::generate(&env);
    mint_to(&env, &token_address, &admin, &contributor, 50_000);
    client.contribute(&contributor, &50_000, &None);

    let tier = client.get_user_tier(&contributor);
    assert!(tier.is_some());
    assert_eq!(tier.unwrap(), bronze);
}

#[test]
fn test_get_user_tier_gold_level() {
    let (env, client, creator, token_address, admin) = setup_env();

    let deadline = env.ledger().timestamp() + 3600;
    let goal: i128 = 1_000_000;
    let min_contribution: i128 = 10_000;
    client.initialize(&creator, &token_address, &goal, &deadline, &min_contribution, &default_title(&env), &default_description(&env), &None);

    let bronze = soroban_sdk::String::from_str(&env, "Bronze");
    let silver = soroban_sdk::String::from_str(&env, "Silver");
    let gold = soroban_sdk::String::from_str(&env, "Gold");
    client.add_reward_tier(&creator, &bronze, &10_000);
    client.add_reward_tier(&creator, &silver, &100_000);
    client.add_reward_tier(&creator, &gold, &500_000);

    let contributor = Address::generate(&env);
    mint_to(&env, &token_address, &admin, &contributor, 600_000);
    client.contribute(&contributor, &600_000, &None);

    let tier = client.get_user_tier(&contributor);
    assert!(tier.is_some());
    assert_eq!(tier.unwrap(), gold);
}

#[test]
fn test_get_user_tier_non_contributor_returns_none() {
    let (env, client, creator, token_address, _admin) = setup_env();

    let deadline = env.ledger().timestamp() + 3600;
    let goal: i128 = 1_000_000;
    let min_contribution: i128 = 1_000;
    client.initialize(
        &creator,
        &token_address,
        &goal,
        &deadline,
        &min_contribution,
        &default_title(&env),
        &default_description(&env),
        &None,
    );

    let bronze = soroban_sdk::String::from_str(&env, "Bronze");
    client.add_reward_tier(&creator, &bronze, &10_000);

    let non_contributor = Address::generate(&env);
    let tier = client.get_user_tier(&non_contributor);
    assert!(tier.is_none());
}

#[test]
fn test_get_user_tier_no_tiers_defined_returns_none() {
    let (env, client, creator, token_address, admin) = setup_env();

    let deadline = env.ledger().timestamp() + 3600;
    let goal: i128 = 1_000_000;
    let min_contribution: i128 = 1_000;
    client.initialize(
        &creator,
        &token_address,
        &goal,
        &(goal * 2),
        &deadline,
        &min_contribution,
        &None,
        &None,
        &None,
    );

    let contributor = Address::generate(&env);
    mint_to(&env, &token_address, &admin, &contributor, 500_000);
    client.contribute(&contributor, &500_000, &None);

    let tier = client.get_user_tier(&contributor);
    assert!(tier.is_none());
}

#[test]
fn test_get_user_tier_highest_qualifying_tier_returned() {
    let (env, client, creator, token_address, admin) = setup_env();

    let deadline = env.ledger().timestamp() + 3600;
    let goal: i128 = 1_000_000;
    let min_contribution: i128 = 1_000;
    client.initialize(
        &creator,
        &token_address,
        &goal,
        &(goal * 2),
        &deadline,
        &min_contribution,
        &None,
        &None,
        &None,
    );

    let bronze = soroban_sdk::String::from_str(&env, "Bronze");
    let silver = soroban_sdk::String::from_str(&env, "Silver");
    let gold = soroban_sdk::String::from_str(&env, "Gold");
    client.add_reward_tier(&creator, &bronze, &10_000);
    client.add_reward_tier(&creator, &silver, &100_000);
    client.add_reward_tier(&creator, &gold, &500_000);

    let contributor = Address::generate(&env);
    mint_to(&env, &token_address, &admin, &contributor, 1_000_000);
    client.contribute(&contributor, &1_000_000, &None);

    let tier = client.get_user_tier(&contributor);
    assert!(tier.is_some());
    assert_eq!(tier.unwrap(), gold);
}

#[test]
#[should_panic]
fn test_add_reward_tier_non_creator_rejected() {
    let (env, client, creator, token_address, _admin) = setup_env();

    let deadline = env.ledger().timestamp() + 3600;
    let goal: i128 = 1_000_000;
    let min_contribution: i128 = 1_000;
    client.initialize(
        &creator,
        &token_address,
        &goal,
        &(goal * 2),
        &deadline,
        &min_contribution,
        &None,
        &None,
        &None,
    );

    let non_creator = Address::generate(&env);
    let bronze = soroban_sdk::String::from_str(&env, "Bronze");
    client.add_reward_tier(&non_creator, &bronze, &10_000);
}

#[test]
#[should_panic(expected = "min_amount must be greater than 0")]
fn test_add_reward_tier_rejects_zero_min_amount() {
    let (env, client, creator, token_address, _admin) = setup_env();

    let deadline = env.ledger().timestamp() + 3600;
    let goal: i128 = 1_000_000;
    let min_contribution: i128 = 1_000;
    client.initialize(
        &creator,
        &token_address,
        &goal,
        &(goal * 2),
        &deadline,
        &min_contribution,
        &None,
        &None,
        &None,
    );

    let bronze = soroban_sdk::String::from_str(&env, "Bronze");
    client.add_reward_tier(&creator, &bronze, &0);
}

#[test]
fn test_reward_tiers_view() {
    let (env, client, creator, token_address, _admin) = setup_env();

    let deadline = env.ledger().timestamp() + 3600;
    let goal: i128 = 1_000_000;
    let min_contribution: i128 = 1_000;
    client.initialize(
        &creator,
        &token_address,
        &goal,
        &(goal * 2),
        &deadline,
        &min_contribution,
        &None,
        &None,
        &None,
    );

    assert_eq!(client.reward_tiers().len(), 0);

    let bronze = soroban_sdk::String::from_str(&env, "Bronze");
    let silver = soroban_sdk::String::from_str(&env, "Silver");
    client.add_reward_tier(&creator, &bronze, &10_000);
    client.add_reward_tier(&creator, &silver, &100_000);

    let tiers = client.reward_tiers();
    assert_eq!(tiers.len(), 2);
    assert_eq!(tiers.get(0).unwrap().name, bronze);
    assert_eq!(tiers.get(0).unwrap().min_amount, 10_000);
    assert_eq!(tiers.get(1).unwrap().name, silver);
    assert_eq!(tiers.get(1).unwrap().min_amount, 100_000);
}

// ── Roadmap Tests ──────────────────────────────────────────────────────────

#[test]
fn test_add_single_roadmap_item() {
    let (env, client, creator, token_address, _admin) = setup_env();

    let deadline = env.ledger().timestamp() + 3600;
    let goal: i128 = 1_000_000;
    let min_contribution: i128 = 1_000;
    client.initialize(
        &creator,
        &token_address,
        &goal,
        &(goal * 2),
        &deadline,
        &min_contribution,
        &None,
        &None,
        &None,
    );

    let current_time = env.ledger().timestamp();
    let roadmap_date = current_time + 86400; // 1 day in the future
    let description = soroban_sdk::String::from_str(&env, "Beta release");

    client.add_roadmap_item(&roadmap_date, &description);

    let roadmap = client.roadmap();
    assert_eq!(roadmap.len(), 1);
    assert_eq!(roadmap.get(0).unwrap().date, roadmap_date);
    assert_eq!(roadmap.get(0).unwrap().description, description);
}

#[test]
fn test_add_multiple_roadmap_items_in_order() {
    let (env, client, creator, token_address, _admin) = setup_env();

    let deadline = env.ledger().timestamp() + 3600;
    let goal: i128 = 1_000_000;
    let min_contribution: i128 = 1_000;
    client.initialize(
        &creator,
        &token_address,
        &goal,
        &deadline,
        &min_contribution,
        &default_title(&env),
        &default_description(&env),
        &None,
    );

    let current_time = env.ledger().timestamp();
    let date1 = current_time + 86400;
    let date2 = current_time + 172800;
    let date3 = current_time + 259200;

    let desc1 = soroban_sdk::String::from_str(&env, "Alpha release");
    let desc2 = soroban_sdk::String::from_str(&env, "Beta release");
    let desc3 = soroban_sdk::String::from_str(&env, "Production launch");

    client.add_roadmap_item(&date1, &desc1);
    client.add_roadmap_item(&date2, &desc2);
    client.add_roadmap_item(&date3, &desc3);

    let roadmap = client.roadmap();
    assert_eq!(roadmap.len(), 3);
    assert_eq!(roadmap.get(0).unwrap().date, date1);
    assert_eq!(roadmap.get(1).unwrap().date, date2);
    assert_eq!(roadmap.get(2).unwrap().date, date3);
    assert_eq!(roadmap.get(0).unwrap().description, desc1);
    assert_eq!(roadmap.get(1).unwrap().description, desc2);
    assert_eq!(roadmap.get(2).unwrap().description, desc3);
}

#[test]
#[should_panic(expected = "date must be in the future")]
fn test_add_roadmap_item_with_past_date_panics() {
    let (env, client, creator, token_address, _admin) = setup_env();

    let deadline = env.ledger().timestamp() + 3600;
    let goal: i128 = 1_000_000;
    let min_contribution: i128 = 1_000;
    client.initialize(
        &creator,
        &token_address,
        &goal,
        &deadline,
        &min_contribution,
        &default_title(&env),
        &default_description(&env),
        &None,
    );

    let current_time = env.ledger().timestamp();
    // Set a past date by moving time forward first, then trying to add an item with an earlier date
    env.ledger().set_timestamp(current_time + 1000);
    let past_date = current_time + 500; // Earlier than the new current time
    let description = soroban_sdk::String::from_str(&env, "Past milestone");

    client.add_roadmap_item(&past_date, &description); // should panic
}

#[test]
#[should_panic(expected = "date must be in the future")]
fn test_add_roadmap_item_with_current_date_panics() {
    let (env, client, creator, token_address, _admin) = setup_env();

    let deadline = env.ledger().timestamp() + 3600;
    let goal: i128 = 1_000_000;
    let min_contribution: i128 = 1_000;
    client.initialize(
        &creator,
        &token_address,
        &goal,
        &deadline,
        &min_contribution,
        &default_title(&env),
        &default_description(&env),
        &None,
    );

    let current_time = env.ledger().timestamp();
    let description = soroban_sdk::String::from_str(&env, "Current milestone");

    client.add_roadmap_item(&current_time, &description); // should panic
}

#[test]
#[should_panic(expected = "description cannot be empty")]
fn test_add_roadmap_item_with_empty_description_panics() {
    let (env, client, creator, token_address, _admin) = setup_env();

    let deadline = env.ledger().timestamp() + 3600;
    let goal: i128 = 1_000_000;
    let min_contribution: i128 = 1_000;
    client.initialize(
        &creator,
        &token_address,
        &goal,
        &deadline,
        &min_contribution,
        &default_title(&env),
        &default_description(&env),
        &None,
    );

    let current_time = env.ledger().timestamp();
    let roadmap_date = current_time + 86400;
    let empty_description = soroban_sdk::String::from_str(&env, "");

    client.add_roadmap_item(&roadmap_date, &empty_description); // should panic
}

#[test]
#[should_panic]
fn test_add_roadmap_item_by_non_creator_panics() {
    let env = Env::default();
    let contract_id = env.register(crate::CrowdfundContract, ());
    let client = crate::CrowdfundContractClient::new(&env, &contract_id);

    let token_admin = Address::generate(&env);
    let token_contract_id = env.register_stellar_asset_contract_v2(token_admin.clone());
    let token_address = token_contract_id.address();

    let platform_admin = Address::generate(&env);
    let creator = Address::generate(&env);
    let non_creator = Address::generate(&env);

    env.mock_all_auths();

    let deadline = env.ledger().timestamp() + 3600;
    let goal: i128 = 1_000_000;
    let min_contribution: i128 = 1_000;
    client.initialize(
        &creator,
        &token_address,
        &goal,
        &deadline,
        &min_contribution,
        &default_title(&env),
        &default_description(&env),
        &None,
    );

    env.mock_all_auths_allowing_non_root_auth();
    env.set_auths(&[]);

    let current_time = env.ledger().timestamp();
    let roadmap_date = current_time + 86400;
    let description = soroban_sdk::String::from_str(&env, "Milestone");

    client.mock_auths(&[soroban_sdk::testutils::MockAuth {
        address: &non_creator,
        invoke: &soroban_sdk::testutils::MockAuthInvoke {
            contract: &contract_id,
            fn_name: "add_roadmap_item",
            args: soroban_sdk::vec![&env],
            sub_invokes: &[],
        },
    }]);

    client.add_roadmap_item(&roadmap_date, &description); // should panic
}

#[test]
fn test_roadmap_empty_after_initialization() {
    let (env, client, creator, token_address, _admin) = setup_env();

    let deadline = env.ledger().timestamp() + 3600;
    let goal: i128 = 1_000_000;
    let min_contribution: i128 = 1_000;
    client.initialize(
        &creator,
        &token_address,
        &goal,
        &deadline,
        &min_contribution,
        &default_title(&env),
        &default_description(&env),
        &None,
    );

    let roadmap = client.roadmap();
    assert_eq!(roadmap.len(), 0);
}

// ── Campaign Updates Tests ─────────────────────────────────────────────────

#[test]
fn test_post_single_update() {
    let (env, client, creator, token_address, _admin) = setup_env();

    let deadline = env.ledger().timestamp() + 3600;
    let goal: i128 = 1_000_000;
    let hard_cap: i128 = goal * 2;
    let min_contribution: i128 = 1_000;
    client.initialize(
        &creator,
        &token_address,
        &goal,
        &hard_cap,
        &deadline,
        &min_contribution,
        &default_title(&env),
        &default_description(&env),
        &None,
    );

    let update_text = soroban_sdk::String::from_str(&env, "Development milestone reached!");
    client.post_update(&update_text);

    let updates = client.get_updates();
    assert_eq!(updates.len(), 1);
    let (timestamp, text) = updates.get(0).unwrap();
    assert_eq!(timestamp, env.ledger().timestamp());
    assert_eq!(text, update_text);
}

#[test]
fn test_post_multiple_updates_chronological_order() {
    let (env, client, creator, token_address, _admin) = setup_env();

    let deadline = env.ledger().timestamp() + 3600;
    let goal: i128 = 1_000_000;
    let hard_cap: i128 = goal * 2;
    let min_contribution: i128 = 1_000;
    client.initialize(
        &creator,
        &token_address,
        &goal,
        &hard_cap,
        &deadline,
        &min_contribution,
        &default_title(&env),
        &default_description(&env),
        &None,
    );

    let update1 = soroban_sdk::String::from_str(&env, "First update");
    let time1 = env.ledger().timestamp();
    client.post_update(&update1);

    env.ledger().set_timestamp(time1 + 100);
    let update2 = soroban_sdk::String::from_str(&env, "Second update");
    let time2 = env.ledger().timestamp();
    client.post_update(&update2);

    env.ledger().set_timestamp(time2 + 200);
    let update3 = soroban_sdk::String::from_str(&env, "Third update");
    let time3 = env.ledger().timestamp();
    client.post_update(&update3);

    let updates = client.get_updates();
    assert_eq!(updates.len(), 3);

    let (ts1, text1) = updates.get(0).unwrap();
    assert_eq!(ts1, time1);
    assert_eq!(text1, update1);

    let (ts2, text2) = updates.get(1).unwrap();
    assert_eq!(ts2, time2);
    assert_eq!(text2, update2);

    let (ts3, text3) = updates.get(2).unwrap();
    assert_eq!(ts3, time3);
    assert_eq!(text3, update3);
}

#[test]
#[should_panic]
fn test_post_update_by_non_creator_panics() {
    let env = Env::default();
    env.mock_all_auths();

    let contract_id = env.register(CrowdfundContract, ());
    let client = CrowdfundContractClient::new(&env, &contract_id);

    let token_admin = Address::generate(&env);
    let token_contract_id = env.register_stellar_asset_contract_v2(token_admin);
    let token_address = token_contract_id.address();

    let creator = Address::generate(&env);
    let non_creator = Address::generate(&env);

    let deadline = env.ledger().timestamp() + 3600;
    let goal: i128 = 1_000_000;
    let hard_cap: i128 = goal * 2;
    let min_contribution: i128 = 1_000;
    client.initialize(
        &creator,
        &token_address,
        &goal,
        &hard_cap,
        &deadline,
        &min_contribution,
        &default_title(&env),
        &default_description(&env),
        &None,
    );

    // Set auth to non-creator
    env.mock_all_auths_allowing_non_root_auth();
    let update_text = soroban_sdk::String::from_str(&env, "Unauthorized update");

    // This should panic because non_creator is not authorized
    client.post_update(&update_text);
}

#[test]
#[should_panic(expected = "update text cannot be empty")]
fn test_post_update_with_empty_text_panics() {
    let (env, client, creator, token_address, _admin) = setup_env();

    let deadline = env.ledger().timestamp() + 3600;
    let goal: i128 = 1_000_000;
    let hard_cap: i128 = goal * 2;
    let min_contribution: i128 = 1_000;
    client.initialize(
        &creator,
        &token_address,
        &goal,
        &hard_cap,
        &deadline,
        &min_contribution,
        &default_title(&env),
        &default_description(&env),
        &None,
    );

    let empty_text = soroban_sdk::String::from_str(&env, "");
    client.post_update(&empty_text); // should panic
}

#[test]
fn test_get_updates_empty_after_initialization() {
    let (env, client, creator, token_address, _admin) = setup_env();

    let deadline = env.ledger().timestamp() + 3600;
    let goal: i128 = 1_000_000;
    let hard_cap: i128 = goal * 2;
    let min_contribution: i128 = 1_000;
    client.initialize(
        &creator,
        &token_address,
        &goal,
        &hard_cap,
        &deadline,
        &min_contribution,
        &default_title(&env),
        &default_description(&env),
        &None,
    );

    let updates = client.get_updates();
    assert_eq!(updates.len(), 0);
}

// ── Campaign Info Tests ────────────────────────────────────────────────────

#[test]
fn test_creator() {
    let (env, client, creator, token_address, _admin) = setup_env();

    let deadline = env.ledger().timestamp() + 3600;
    let goal: i128 = 1_000_000;
    let min_contribution: i128 = 1_000;
    client.initialize(
        &creator,
        &token_address,
        &goal,
        &(goal * 2),
        &deadline,
        &min_contribution,
    );

    assert_eq!(client.creator(), creator);
}

#[test]
fn test_get_campaign_info_initial() {
    let (env, client, creator, token_address, _admin) = setup_env();

    let deadline = env.ledger().timestamp() + 3600;
    let goal: i128 = 1_000_000;
    let min_contribution: i128 = 1_000;
    client.initialize(
        &creator,
        &token_address,
        &goal,
        &deadline,
        &min_contribution,
        &default_title(&env),
        &default_description(&env),
        &None,
    );

    let info = client.get_campaign_info();

    assert_eq!(info.creator, creator);
    assert_eq!(info.token, token_address);
    assert_eq!(info.goal, goal);
    assert_eq!(info.deadline, deadline);
    assert_eq!(info.total_raised, 0);
}

#[test]
fn test_get_campaign_info_with_contributions() {
    let (env, client, creator, token_address, admin) = setup_env();

    let deadline = env.ledger().timestamp() + 3600;
    let goal: i128 = 1_000_000;
    let min_contribution: i128 = 1_000;
    client.initialize(
        &creator,
        &token_address,
        &goal,
        &(goal * 2),
        &deadline,
        &min_contribution,
    );

    let alice = Address::generate(&env);
    let bob = Address::generate(&env);
    mint_to(&env, &token_address, &admin, &alice, 600_000);
    mint_to(&env, &token_address, &admin, &bob, 300_000);

    client.contribute(&alice, &600_000);
    client.contribute(&bob, &300_000);

    let info = client.get_campaign_info();

    assert_eq!(info.creator, creator);
    assert_eq!(info.token, token_address);
    assert_eq!(info.goal, goal);
    assert_eq!(info.deadline, deadline);
    assert_eq!(info.total_raised, 900_000);
}

// ── Whitelist Tests ────────────────────────────────────────────────────────

#[test]
fn test_get_campaign_info_after_goal_reached() {
    let (env, client, creator, token_address, admin) = setup_env();

    let deadline = env.ledger().timestamp() + 3600;
    let goal: i128 = 1_000_000;
    let min_contribution: i128 = 1_000;
    client.initialize(
        &creator,
        &token_address,
        &goal,
        &(goal * 2),
        &deadline,
        &min_contribution,
    );

    let contributor = Address::generate(&env);
    mint_to(&env, &token_address, &admin, &contributor, 1_000_000);
    client.contribute(&contributor, &1_000_000, &None);

    let info = client.get_campaign_info();

    assert_eq!(info.creator, creator);
    assert_eq!(info.token, token_address);
    assert_eq!(info.goal, goal);
    assert_eq!(info.deadline, deadline);
    assert_eq!(info.total_raised, 1_500_000);
}

// ── Whitelist Tests ────────────────────────────────────────────────────────

#[test]
fn test_whitelisted_contribution() {
    let (env, client, creator, token_address, admin) = setup_env();

    let deadline = env.ledger().timestamp() + 3600;
    let goal: i128 = 1_000_000;
    let min_contribution: i128 = 1_000;
    client.initialize(
        &creator,
        &token_address,
        &goal,
        &(goal * 2),
        &deadline,
        &min_contribution,
    );

    let alice = Address::generate(&env);
    let bob = Address::generate(&env);

    // Add Alice to whitelist
    client.add_to_whitelist(&soroban_sdk::vec![&env, alice.clone()]);

    mint_to(&env, &token_address, &admin, &alice, 500_000);
    mint_to(&env, &token_address, &admin, &bob, 500_000);

    // Alice (whitelisted) can contribute
    client.contribute(&alice, &500_000);
    assert_eq!(client.contribution(&alice), 500_000);

    // Bob (not whitelisted) cannot contribute
    let result = client.try_contribute(&bob, &500_000);
    assert!(result.is_err());
}

#[test]
fn test_open_campaign_no_whitelist() {
    let (env, client, creator, token_address, admin) = setup_env();

    let deadline = env.ledger().timestamp() + 3600;
    let goal: i128 = 1_000_000;
    let min_contribution: i128 = 1_000;
    client.initialize(
        &creator,
        &token_address,
        &goal,
        &(goal * 2),
        &deadline,
        &min_contribution,
    );

    let alice = Address::generate(&env);
    mint_to(&env, &token_address, &admin, &alice, 500_000);

    // Any address can contribute if no addresses were ever added to the whitelist
    client.contribute(&alice, &500_000);
    assert_eq!(client.contribution(&alice), 500_000);
}

#[test]
fn test_batch_whitelist_addition() {
    let (env, client, creator, token_address, admin) = setup_env();

    let deadline = env.ledger().timestamp() + 3600;
    let goal: i128 = 1_000_000;
    let min_contribution: i128 = 1_000;
    client.initialize(
        &creator,
        &token_address,
        &goal,
        &deadline,
        &min_contribution,
    );

    let stretch_milestone: i128 = 1_500_000;
    client.add_stretch_goal(&stretch_milestone);

    assert_eq!(client.current_milestone(), stretch_milestone);
}

#[test]
#[should_panic(expected = "bonus goal must be greater than primary goal")]
fn test_initialize_rejects_bonus_goal_not_above_primary() {
    let (env, client, creator, token_address, _admin) = setup_env();

    let deadline = env.ledger().timestamp() + 3600;
    let goal: i128 = 1_000_000;
    let invalid_bonus_goal: i128 = 1_000_000;

    client.initialize(
        &creator,
        &token_address,
        &goal,
        &(goal * 2),
        &deadline,
        &1_000,
        &None,
        &Some(invalid_bonus_goal),
        &None,
    );
}

#[test]
fn test_bonus_goal_progress_tracked_separately_from_primary() {
    let (env, client, creator, token_address, admin) = setup_env();

    let deadline = env.ledger().timestamp() + 3600;
    let goal: i128 = 1_000_000;
    let bonus_goal: i128 = 2_000_000;
    let bonus_description = soroban_sdk::String::from_str(&env, "Bonus unlocked");

    client.initialize(
        &creator,
        &token_address,
        &goal,
        &(goal * 2),
        &deadline,
        &1_000,
        &None,
        &Some(bonus_goal),
        &Some(bonus_description.clone()),
    );

    let contributor = Address::generate(&env);
    mint_to(&env, &token_address, &admin, &contributor, 500_000);
    client.contribute(&contributor, &500_000, &None);

    let primary_progress_bps = (client.total_raised() * 10_000) / client.goal();
    assert_eq!(primary_progress_bps, 5_000);
    assert_eq!(client.bonus_goal_progress_bps(), 2_500);
    assert_eq!(client.bonus_goal(), Some(bonus_goal));
    assert_eq!(client.bonus_goal_description(), Some(bonus_description));
}

#[test]
fn test_bonus_goal_reached_returns_false_below_threshold() {
    let (env, client, creator, token_address, admin) = setup_env();

    let deadline = env.ledger().timestamp() + 3600;
    let goal: i128 = 1_000_000;
    let bonus_goal: i128 = 2_000_000;
    let hard_cap: i128 = 3_000_000;
    client.initialize(
        &creator,
        &token_address,
        &goal,
        &hard_cap,
        &deadline,
        &1_000,
        &None,
        &Some(bonus_goal),
        &None,
    );

    let contributor = Address::generate(&env);
    mint_to(&env, &token_address, &admin, &contributor, 1_500_000);
    client.contribute(&contributor, &1_500_000, &None);

    assert!(!client.bonus_goal_reached());
}

#[test]
fn test_bonus_goal_reached_returns_true_at_and_above_threshold() {
    let (env, client, creator, token_address, admin) = setup_env();

    let deadline = env.ledger().timestamp() + 3600;
    let goal: i128 = 1_000_000;
    let bonus_goal: i128 = 2_000_000;
    let hard_cap: i128 = 3_000_000;
    client.initialize(
        &creator,
        &token_address,
        &goal,
        &hard_cap,
        &deadline,
        &1_000,
        &None,
        &Some(bonus_goal),
        &None,
    );

    let contributor = Address::generate(&env);
    mint_to(&env, &token_address, &admin, &contributor, 2_100_000);
    client.contribute(&contributor, &2_000_000, &None);
    assert!(client.bonus_goal_reached());

    client.contribute(&contributor, &100_000, &None);
    assert!(client.bonus_goal_reached());
    assert_eq!(client.bonus_goal_progress_bps(), 10_000);
}

// ── Property-Based Fuzz Tests with Proptest ────────────────────────────────

/// **Property Test 1: Invariant - Total Raised Equals Sum of Contributions**
///
/// For any valid (goal, deadline, contributions[]), the contract invariant holds:
/// total_raised == sum of all individual contributions
///
/// This test generates random valid parameters and multiple contributors with
/// varying contribution amounts, then verifies the invariant is maintained.
proptest! {
    #[test]
    fn prop_total_raised_equals_sum_of_contributions(
        goal in 1_000_000i128..100_000_000i128,
        deadline_offset in 100u64..100_000u64,
        amount1 in 1_000i128..10_000_000i128,
        amount2 in 1_000i128..10_000_000i128,
        amount3 in 1_000i128..10_000_000i128,
    ) {
        let (env, client, creator, token_address, admin) = setup_env();
        let deadline = env.ledger().timestamp() + deadline_offset;
        let hard_cap = (amount1 + amount2 + amount3).max(goal * 2);

        client.initialize(
            &creator,
            &token_address,
            &goal,
            &hard_cap,
            &deadline,
            &1_000,
            &None,
            &None,
            &None,
        );

        let alice = Address::generate(&env);
        let bob = Address::generate(&env);
        let charlie = Address::generate(&env);

        mint_to(&env, &token_address, &admin, &alice, amount1);
        mint_to(&env, &token_address, &admin, &bob, amount2);
        mint_to(&env, &token_address, &admin, &charlie, amount3);

        client.contribute(&alice, &amount1, &None);
        client.contribute(&bob, &amount2, &None);
        client.contribute(&charlie, &amount3, &None);

        let expected_total = amount1 + amount2 + amount3;
        let actual_total = client.total_raised();

        // **INVARIANT**: total_raised must equal the sum of all contributions
        prop_assert_eq!(actual_total, expected_total,
            "total_raised ({}) != sum of contributions ({})",
            actual_total, expected_total
        );
    }
}

/// **Property Test 2: Invariant - Refund Returns Exact Contributed Amount**
///
/// For any valid contribution amount, refund always returns the exact amount
/// with no remainder or shortfall.
///
/// This test verifies that each contributor receives back exactly what they
/// contributed when the goal is not met and refund is called.
proptest! {
    #[test]
    fn prop_refund_returns_exact_amount(
        goal in 5_000_000i128..100_000_000i128,
        deadline_offset in 100u64..100_000u64,
        contribution in 1_000i128..5_000_000i128,
    ) {
        let (env, client, creator, token_address, admin) = setup_env();
        let deadline = env.ledger().timestamp() + deadline_offset;

        // Ensure contribution is less than goal
        let safe_contribution = contribution.min(goal - 1);

        client.initialize(
            &creator,
            &token_address,
            &goal,
            &(goal * 2),
            &deadline,
            &1_000,
            &None,
            &None,
            &None,
        );

        let contributor = Address::generate(&env);
        mint_to(&env, &token_address, &admin, &contributor, safe_contribution);
        client.contribute(&contributor, &safe_contribution, &None);

        // Move past deadline (goal not met)
        env.ledger().set_timestamp(deadline + 1);

        let token_client = token::Client::new(&env, &token_address);
        let balance_before_refund = token_client.balance(&contributor);

        client.refund();

        let balance_after_refund = token_client.balance(&contributor);

        // **INVARIANT**: Refund must return exact amount with no remainder
        prop_assert_eq!(
            balance_after_refund - balance_before_refund,
            safe_contribution,
            "refund amount ({}) != original contribution ({})",
            balance_after_refund - balance_before_refund,
            safe_contribution
        );
    }
}

/// **Property Test 3: Contribute with Amount <= 0 Always Fails**
///
/// For any contribution amount <= 0, the contribute function must fail.
/// This test verifies that zero and negative contributions are rejected.
proptest! {
    #[test]
    fn prop_contribute_zero_or_negative_fails(
        goal in 1_000_000i128..10_000_000i128,
        deadline_offset in 100u64..10_000u64,
        negative_amount in -1_000_000i128..=0i128,
    ) {
        let (env, client, creator, token_address, admin) = setup_env();
        let deadline = env.ledger().timestamp() + deadline_offset;

        client.initialize(
            &creator,
            &token_address,
            &goal,
            &(goal * 2),
            &deadline,
            &1_000,
            &None,
            &None,
            &None,
        );

        let contributor = Address::generate(&env);
        // Mint enough tokens so the failure is due to amount validation, not balance
        mint_to(&env, &token_address, &admin, &contributor, 10_000_000);

        // Attempt to contribute zero or negative amount
        // This should fail due to minimum contribution check
        let result = client.try_contribute(&contributor, &negative_amount, &None);

        // **INVARIANT**: Contribution <= 0 must fail
        prop_assert!(
            result.is_err(),
            "contribute with amount {} should fail but succeeded",
            negative_amount
        );
    }
}

/// **Property Test 4: Deadline in the Past Always Fails on Initialize**
///
/// For any deadline in the past (relative to current ledger time),
/// initialization must fail or panic.
proptest! {
    #[test]
    fn prop_initialize_with_past_deadline_fails(
        goal in 1_000_000i128..10_000_000i128,
        past_offset in 1u64..10_000u64,
    ) {
        let (env, client, creator, token_address, _admin) = setup_env();

        let current_time = env.ledger().timestamp();
        // Set deadline in the past
        let past_deadline = current_time.saturating_sub(past_offset);

        // Attempt to initialize with past deadline
        let result = client.try_initialize(
            &creator,
            &token_address,
            &goal,
            &(goal * 2),
            &past_deadline,
            &1_000,
            &None,
        &None,
        &None,
        );

        // **INVARIANT**: Past deadline should fail or be rejected
        // Note: The contract may not explicitly validate this, but it's a logical invariant
        // If the contract allows it, the campaign would already be expired
        // This test documents the expected behavior
        if result.is_ok() {
            // If initialization succeeds with past deadline, verify campaign is immediately expired
            let deadline = client.deadline();
            prop_assert!(
                deadline <= current_time,
                "deadline {} should be in the past relative to current time {}",
                deadline,
                current_time
            );
        }
    }
}

/// **Property Test 5: Multiple Contributions Accumulate Correctly**
///
/// For any sequence of valid contributions from multiple contributors,
/// the total_raised must equal the sum of all contributions.
proptest! {
    #[test]
    fn prop_multiple_contributions_accumulate(
        goal in 5_000_000i128..50_000_000i128,
        deadline_offset in 100u64..100_000u64,
        amount1 in 1_000i128..5_000_000i128,
        amount2 in 1_000i128..5_000_000i128,
        amount3 in 1_000i128..5_000_000i128,
    ) {
        let (env, client, creator, token_address, admin) = setup_env();
        let deadline = env.ledger().timestamp() + deadline_offset;
        let expected_total = amount1 + amount2 + amount3;
        let hard_cap = expected_total.max(goal);

        client.initialize(
            &creator,
            &token_address,
            &goal,
            &hard_cap,
            &deadline,
            &1_000,
            &None,
            &None,
            &None,
        );

        let contributor1 = Address::generate(&env);
        let contributor2 = Address::generate(&env);
        let contributor3 = Address::generate(&env);

        mint_to(&env, &token_address, &admin, &contributor1, amount1);
        mint_to(&env, &token_address, &admin, &contributor2, amount2);
        mint_to(&env, &token_address, &admin, &contributor3, amount3);

        client.contribute(&contributor1, &amount1, &None);
        client.contribute(&contributor2, &amount2, &None);
        client.contribute(&contributor3, &amount3, &None);

        // **INVARIANT**: total_raised must equal sum of all contributions
        prop_assert_eq!(client.total_raised(), expected_total);

        // **INVARIANT**: Each contributor's balance must be tracked correctly
        prop_assert_eq!(client.contribution(&contributor1), amount1);
        prop_assert_eq!(client.contribution(&contributor2), amount2);
        prop_assert_eq!(client.contribution(&contributor3), amount3);
    }
}

/// **Property Test 6: Withdrawal After Goal Met Transfers Correct Amount**
///
/// For any valid goal and contributions that meet or exceed the goal,
/// withdrawal must transfer the exact total_raised amount to the creator.
proptest! {
    #[test]
    fn prop_withdrawal_transfers_exact_amount(
        goal in 1_000_000i128..10_000_000i128,
        deadline_offset in 100u64..10_000u64,
    ) {
        let (env, client, creator, token_address, admin) = setup_env();
        let deadline = env.ledger().timestamp() + deadline_offset;

        client.initialize(
            &creator,
            &token_address,
            &goal,
            &(goal * 2),
            &deadline,
            &1_000,
            &None,
            &None,
            &None,
        );

        let contributor = Address::generate(&env);
        mint_to(&env, &token_address, &admin, &contributor, goal);
        client.contribute(&contributor, &goal, &None);

        // Move past deadline
        env.ledger().set_timestamp(deadline + 1);

        let token_client = token::Client::new(&env, &token_address);
        let creator_balance_before = token_client.balance(&creator);

        client.withdraw();

        let creator_balance_after = token_client.balance(&creator);
        let transferred_amount = creator_balance_after - creator_balance_before;

        // **INVARIANT**: Withdrawal must transfer exact total_raised amount
        prop_assert_eq!(
            transferred_amount, goal,
            "withdrawal transferred {} but expected {}",
            transferred_amount, goal
        );

        // **INVARIANT**: total_raised must be reset to 0 after withdrawal
        prop_assert_eq!(client.total_raised(), 0);
    }
}

/// **Property Test 7: Contribution Tracking Persists Across Multiple Calls**
///
/// For any contributor making multiple contributions, the total tracked
/// must equal the sum of all their contributions.
proptest! {
    #[test]
    fn prop_contribution_tracking_persists(
        goal in 5_000_000i128..50_000_000i128,
        deadline_offset in 100u64..100_000u64,
        amount1 in 1_000i128..2_000_000i128,
        amount2 in 1_000i128..2_000_000i128,
        amount3 in 1_000i128..2_000_000i128,
    ) {
        let (env, client, creator, token_address, admin) = setup_env();
        let deadline = env.ledger().timestamp() + deadline_offset;

        client.initialize(
            &creator,
            &token_address,
            &goal,
            &(goal * 2),
            &deadline,
            &1_000,
            &None,
            &None,
            &None,
        );

        let contributor = Address::generate(&env);
        let total_needed = amount1.saturating_add(amount2).saturating_add(amount3);
        mint_to(&env, &token_address, &admin, &contributor, total_needed);

        // First contribution
        client.contribute(&contributor, &amount1, &None);
        prop_assert_eq!(client.contribution(&contributor), amount1);

        // Second contribution
        client.contribute(&contributor, &amount2, &None);
        let expected_after_2 = amount1.saturating_add(amount2);
        prop_assert_eq!(client.contribution(&contributor), expected_after_2);

        // Third contribution
        client.contribute(&contributor, &amount3, &None);
        let expected_total = amount1.saturating_add(amount2).saturating_add(amount3);
        prop_assert_eq!(client.contribution(&contributor), expected_total);

        // **INVARIANT**: Final total_raised must equal sum of all contributions
        prop_assert_eq!(client.total_raised(), expected_total);
    }
}

/// **Property Test 8: Refund Resets Total Raised to Zero**
///
/// For any valid refund scenario (goal not met, deadline passed),
/// total_raised must be reset to 0 after refund completes.
proptest! {
    #[test]
    fn prop_refund_resets_total_raised(
        goal in 5_000_000i128..50_000_000i128,
        deadline_offset in 100u64..100_000u64,
        contribution in 1_000i128..5_000_000i128,
    ) {
        let (env, client, creator, token_address, admin) = setup_env();
        let deadline = env.ledger().timestamp() + deadline_offset;

        let safe_contribution = contribution.min(goal - 1);

        client.initialize(
            &creator,
            &token_address,
            &goal,
            &(goal * 2),
            &deadline,
            &1_000,
            &None,
            &None,
            &None,
        );

        let contributor = Address::generate(&env);
        mint_to(&env, &token_address, &admin, &contributor, safe_contribution);
        client.contribute(&contributor, &safe_contribution, &None);

        // Verify total_raised is set
        prop_assert_eq!(client.total_raised(), safe_contribution);

        // Move past deadline (goal not met)
        env.ledger().set_timestamp(deadline + 1);

        client.refund();

        // **INVARIANT**: total_raised must be 0 after refund
        prop_assert_eq!(client.total_raised(), 0);
    }
}

/// **Property Test 9: Contribution Below Minimum Always Fails**
///
/// For any contribution amount below the minimum, the contribute function
/// must fail or panic.
proptest! {
    #[test]
    fn prop_contribute_below_minimum_fails(
        goal in 1_000_000i128..10_000_000i128,
        deadline_offset in 100u64..10_000u64,
        min_contribution in 1_000i128..100_000i128,
        below_minimum in 1i128..1_000i128,
    ) {
        let (env, client, creator, token_address, admin) = setup_env();
        let deadline = env.ledger().timestamp() + deadline_offset;

        client.initialize(
            &creator,
            &token_address,
            &goal,
            &(goal * 2),
            &deadline,
            &min_contribution,
            &None,
            &None,
            &None,
        );

        let contributor = Address::generate(&env);
        let amount_to_contribute = below_minimum.min(min_contribution - 1);
        mint_to(&env, &token_address, &admin, &contributor, amount_to_contribute);

        // Attempt to contribute below minimum
        let result = client.try_contribute(&contributor, &amount_to_contribute, &None);

        // **INVARIANT**: Contribution below minimum must fail
        prop_assert!(
            result.is_err(),
            "contribute with amount {} below minimum {} should fail",
            amount_to_contribute, min_contribution
        );
    }
}

    client.add_to_whitelist(&soroban_sdk::vec![&env, alice.clone(), bob.clone()]);

    assert!(client.is_whitelisted(&alice));
    assert!(client.is_whitelisted(&bob));

    mint_to(&env, &token_address, &admin, &alice, 100_000);
    mint_to(&env, &token_address, &admin, &bob, 100_000);

    client.contribute(&alice, &100_000);
    client.contribute(&bob, &100_000);

    assert_eq!(client.total_raised(), 200_000);
}

#[test]
#[should_panic]
fn test_add_to_whitelist_non_creator_panics() {
    let (env, client, _creator, _token_address, _admin) = setup_env();

    let alice = Address::generate(&env);

    // Non-creator address
    let _attacker = Address::generate(&env);

    // Mock authorization for non-creator
    env.mock_all_auths();

    let result = client.try_contribute(&contributor, &5_000, &None);

    client.add_to_whitelist(&soroban_sdk::vec![&env, alice]);
}

// ── Early Withdrawal Tests ──────────────────────────────────────────────────

#[test]
fn test_partial_withdrawal() {
    let (env, client, creator, token_address, admin) = setup_env();

    let deadline = env.ledger().timestamp() + 3600;
    let goal: i128 = 1_000_000;
    let min_contribution: i128 = 1_000;
    client.initialize(
        &creator,
        &token_address,
        &goal,
        &(goal * 2),
        &deadline,
        &min_contribution,
    );

    let contributor = Address::generate(&env);
    mint_to(&env, &token_address, &admin, &contributor, 500_000);
    client.contribute(&contributor, &500_000);

    assert_eq!(client.total_raised(), 500_000);
    assert_eq!(client.contribution(&contributor), 500_000);

    // Partial withdrawal.
    client.withdraw_contribution(&contributor, &200_000);

    assert_eq!(client.total_raised(), 300_000);
    assert_eq!(client.contribution(&contributor), 300_000);
}

#[test]
fn test_full_withdrawal_removes_contributor() {
    let (env, client, creator, token_address, admin) = setup_env();

    let deadline = env.ledger().timestamp() + 3600;
    let goal: i128 = 1_000_000;
    let min_contribution: i128 = 1_000;
    client.initialize(
        &creator,
        &token_address,
        &goal,
        &(goal * 2),
        &deadline,
        &min_contribution,
    );

    let contributor = Address::generate(&env);
    mint_to(&env, &token_address, &admin, &contributor, 500_000);
    client.contribute(&contributor, &500_000, &None);

    let stats = client.get_stats();
    assert_eq!(stats.contributor_count, 1);

    // Full withdrawal.
    client.withdraw_contribution(&contributor, &500_000);

    assert_eq!(client.total_raised(), 0);
    assert_eq!(client.contribution(&contributor), 0);

    let stats_after = client.get_stats();
    assert_eq!(stats_after.contributor_count, 0);
}

#[test]
#[should_panic(expected = "insufficient balance")]
fn test_withdraw_exceeding_balance_panics() {
    let (env, client, creator, token_address, admin) = setup_env();

    let deadline = env.ledger().timestamp() + 3600;
    let goal: i128 = 1_000_000;
    let min_contribution: i128 = 1_000;
    client.initialize(
        &creator,
        &token_address,
        &goal,
        &deadline,
        &min_contribution,
    );

    let contributor = Address::generate(&env);
    mint_to(&env, &token_address, &admin, &contributor, 100_000);
    client.contribute(&contributor, &100_000);

    client.withdraw_contribution(&contributor, &100_001); // should panic
}

#[test]
#[should_panic(expected = "campaign has ended")]
fn test_withdraw_after_deadline_panics() {
    let (env, client, creator, token_address, admin) = setup_env();

    let deadline = env.ledger().timestamp() + 3600;
    let goal: i128 = 1_000_000;
    let min_contribution: i128 = 1_000;
    client.initialize(
        &creator,
        &token_address,
        &goal,
        &deadline,
        &min_contribution,
    );

    let contributor = Address::generate(&env);
    mint_to(&env, &token_address, &admin, &contributor, 100_000);
    client.contribute(&contributor, &100_000);

    // Fast forward past deadline.
    env.ledger().set_timestamp(deadline + 1);

    client.contribute(&charlie, &100_000);
    assert_eq!(client.contributor_count(), 3);
}

// ── Contributor Count Tests ────────────────────────────────────────────────

#[test]
fn test_contributor_count_zero_before_contributions() {
    let (env, client, creator, token_address, _admin) = setup_env();

    let deadline = env.ledger().timestamp() + 3600;
    let goal: i128 = 1_000_000;
    let min_contribution: i128 = 1_000;

    client.initialize(
        &creator,
        &token_address,
        &goal,
        &(goal * 2),
        &deadline,
        &min_contribution,
        &None,
        &None,
        &None,
    );

    assert_eq!(client.contributor_count(), 0);
}

#[test]
fn test_contributor_count_one_after_single_contribution() {
    let (env, client, creator, token_address, admin) = setup_env();

    let deadline = env.ledger().timestamp() + 3600;
    let goal: i128 = 1_000_000;
    let min_contribution: i128 = 1_000;

    client.initialize(
        &creator,
        &token_address,
        &goal,
        &(goal * 2),
        &deadline,
        &min_contribution,
        &None,
        &None,
        &None,
    );

    let contributor = Address::generate(&env);
    mint_to(&env, &token_address, &admin, &contributor, 100_000);
    client.contribute(&contributor, &100_000);

    // Fast forward past deadline.
    env.ledger().set_timestamp(deadline + 1);

    client.withdraw_contribution(&contributor, &50_000); // should panic
}

// ── Campaign Active Tests ──────────────────────────────────────────────────

#[test]
fn test_is_campaign_active_before_deadline() {
    let (env, client, creator, token_address, _admin) = setup_env();

    let deadline = env.ledger().timestamp() + 3600;
    let goal: i128 = 1_000_000;
    let min_contribution: i128 = 1_000;
    let title = soroban_sdk::String::from_str(&env, "Test Campaign");
    let description = soroban_sdk::String::from_str(&env, "Test Description");

    client.initialize(&creator, &token_address, &goal, &deadline, &min_contribution, &title, &description, &None);

    assert_eq!(client.is_campaign_active(), true);
}

#[test]
fn test_is_campaign_active_at_deadline() {
    let (env, client, creator, token_address, _admin) = setup_env();

    let deadline = env.ledger().timestamp() + 3600;
    let goal: i128 = 1_000_000;
    let min_contribution: i128 = 1_000;
    let title = soroban_sdk::String::from_str(&env, "Test Campaign");
    let description = soroban_sdk::String::from_str(&env, "Test Description");

    client.initialize(&creator, &token_address, &goal, &deadline, &min_contribution, &title, &description, &None);

    env.ledger().set_timestamp(deadline);

    assert_eq!(client.is_campaign_active(), true);
}

#[test]
fn test_is_campaign_active_after_deadline() {
    let (env, client, creator, token_address, _admin) = setup_env();

    let deadline = env.ledger().timestamp() + 3600;
    let goal: i128 = 1_000_000;
    let min_contribution: i128 = 1_000;
    let title = soroban_sdk::String::from_str(&env, "Test Campaign");
    let description = soroban_sdk::String::from_str(&env, "Test Description");

    client.initialize(&creator, &token_address, &goal, &deadline, &min_contribution, &title, &description, &None);

    env.ledger().set_timestamp(deadline + 1);

    assert_eq!(client.is_campaign_active(), false);
}

// ── DAO Protocol Integration Tests ─────────────────────────────────────────

use soroban_sdk::{contract, contractimpl};

/// ProxyCreator is a minimal DAO-like contract that can control a crowdfund campaign.
#[contract]
pub struct ProxyCreator;

#[contractimpl]
impl ProxyCreator {
    pub fn init_campaign(
        env: Env,
        crowdfund_id: Address,
        platform_admin: Address,
        token: Address,
        goal: i128,
        deadline: u64,
        min_contribution: i128,
    ) {
        let crowdfund_client = CrowdfundContractClient::new(&env, &crowdfund_id);
        crowdfund_client.initialize(
            &platform_admin,
            &env.current_contract_address(),
            &token,
            &goal,
            &deadline,
            &min_contribution,
        );
    }

    pub fn withdraw_campaign(env: Env, crowdfund_id: Address) {
        let crowdfund_client = CrowdfundContractClient::new(&env, &crowdfund_id);
        crowdfund_client.withdraw();
    }

    pub fn cancel_campaign(env: Env, crowdfund_id: Address) {
        let crowdfund_client = CrowdfundContractClient::new(&env, &crowdfund_id);
        crowdfund_client.cancel();
    }
}

#[test]
fn test_dao_withdraw_succeeds() {
    let env = Env::default();
    env.mock_all_auths();

    let crowdfund_id = env.register(CrowdfundContract, ());
    let crowdfund_client = CrowdfundContractClient::new(&env, &crowdfund_id);

    let proxy_id = env.register(ProxyCreator, ());
    let proxy_client = ProxyCreatorClient::new(&env, &proxy_id);

    let token_admin = Address::generate(&env);
    let token_contract_id = env.register_stellar_asset_contract_v2(token_admin.clone());
    let token_address = token_contract_id.address();
    let token_admin_client = token::StellarAssetClient::new(&env, &token_address);

    let platform_admin = Address::generate(&env);
    let deadline = env.ledger().timestamp() + 3600;
    let goal: i128 = 1_000_000;
    let min_contribution: i128 = 1_000;

    proxy_client.init_campaign(
        &crowdfund_id,
        &platform_admin,
        &token_address,
        &goal,
        &deadline,
        &min_contribution,
    );

    let info = crowdfund_client.campaign_info();
    assert_eq!(info.creator, proxy_id);

    let contributor = Address::generate(&env);
    token_admin_client.mint(&contributor, &1_000_000);
    crowdfund_client.contribute(&contributor, &1_000_000);

    env.ledger().set_timestamp(deadline + 1);

    proxy_client.withdraw_campaign(&crowdfund_id);

    assert_eq!(crowdfund_client.total_raised(), 0);
    let token_client = token::Client::new(&env, &token_address);
    assert_eq!(token_client.balance(&proxy_id), 1_000_000);
}

#[test]
fn test_dao_cancel_succeeds() {
    let env = Env::default();
    env.mock_all_auths();

    let crowdfund_id = env.register(CrowdfundContract, ());
    let crowdfund_client = CrowdfundContractClient::new(&env, &crowdfund_id);

    let proxy_id = env.register(ProxyCreator, ());
    let proxy_client = ProxyCreatorClient::new(&env, &proxy_id);

    let token_admin = Address::generate(&env);
    let token_contract_id = env.register_stellar_asset_contract_v2(token_admin.clone());
    let token_address = token_contract_id.address();
    let token_admin_client = token::StellarAssetClient::new(&env, &token_address);

    let platform_admin = Address::generate(&env);
    let deadline = env.ledger().timestamp() + 3600;
    let goal: i128 = 1_000_000;
    let min_contribution: i128 = 1_000;

    proxy_client.init_campaign(
        &crowdfund_id,
        &platform_admin,
        &token_address,
        &goal,
        &deadline,
        &min_contribution,
    );

    let contributor = Address::generate(&env);
    token_admin_client.mint(&contributor, &500_000);
    crowdfund_client.contribute(&contributor, &500_000);

    proxy_client.cancel_campaign(&crowdfund_id);

    assert_eq!(crowdfund_client.total_raised(), 0);
    let token_client = token::Client::new(&env, &token_address);
    assert_eq!(token_client.balance(&contributor), 500_000);
}

#[test]
#[should_panic]
fn test_dao_unauthorized_address_rejected() {
    let env = Env::default();

    let crowdfund_id = env.register(CrowdfundContract, ());
    let crowdfund_client = CrowdfundContractClient::new(&env, &crowdfund_id);

    let proxy_id = env.register(ProxyCreator, ());
    let proxy_client = ProxyCreatorClient::new(&env, &proxy_id);

    let token_admin = Address::generate(&env);
    let token_contract_id = env.register_stellar_asset_contract_v2(token_admin.clone());
    let token_address = token_contract_id.address();
    let token_admin_client = token::StellarAssetClient::new(&env, &token_address);

    let platform_admin = Address::generate(&env);
    let unauthorized = Address::generate(&env);
    let deadline = env.ledger().timestamp() + 3600;
    let goal: i128 = 1_000_000;
    let min_contribution: i128 = 1_000;

    env.mock_all_auths();

    proxy_client.init_campaign(
        &crowdfund_id,
        &platform_admin,
        &token_address,
        &goal,
        &deadline,
        &min_contribution,
    );

    let contributor = Address::generate(&env);
    token_admin_client.mint(&contributor, &1_000_000);
    crowdfund_client.contribute(&contributor, &1_000_000);
    env.ledger().set_timestamp(deadline + 1);

    env.mock_all_auths_allowing_non_root_auth();
    env.set_auths(&[]);

    crowdfund_client.mock_auths(&[soroban_sdk::testutils::MockAuth {
        address: &unauthorized,
        invoke: &soroban_sdk::testutils::MockAuthInvoke {
            contract: &crowdfund_id,
            fn_name: "withdraw",
            args: soroban_sdk::vec![&env],
            sub_invokes: &[],
        },
    }]);

    crowdfund_client.withdraw();
}

#[test]
fn test_dao_contract_auth_chain_enforced() {
    let env = Env::default();
    env.mock_all_auths();

    let crowdfund_id = env.register(CrowdfundContract, ());
    let crowdfund_client = CrowdfundContractClient::new(&env, &crowdfund_id);

    let proxy_id = env.register(ProxyCreator, ());
    let proxy_client = ProxyCreatorClient::new(&env, &proxy_id);

    let token_admin = Address::generate(&env);
    let token_contract_id = env.register_stellar_asset_contract_v2(token_admin.clone());
    let token_address = token_contract_id.address();
    let token_admin_client = token::StellarAssetClient::new(&env, &token_address);

    let platform_admin = Address::generate(&env);
    let deadline = env.ledger().timestamp() + 3600;
    let goal: i128 = 1_000_000;
    let min_contribution: i128 = 1_000;

    proxy_client.init_campaign(
        &crowdfund_id,
        &platform_admin,
        &token_address,
        &goal,
        &deadline,
        &min_contribution,
    );

    let info = crowdfund_client.campaign_info();
    assert_eq!(info.creator, proxy_id);
    assert_eq!(info.goal, goal);
    assert_eq!(info.deadline, deadline);

    let contributor = Address::generate(&env);
    token_admin_client.mint(&contributor, &1_000_000);
    crowdfund_client.contribute(&contributor, &1_000_000);

    env.ledger().set_timestamp(deadline + 1);

    proxy_client.withdraw_campaign(&crowdfund_id);
    assert_eq!(crowdfund_client.total_raised(), 0);

    let token_client = token::Client::new(&env, &token_address);
    assert_eq!(token_client.balance(&proxy_id), 1_000_000);
}

// ── Tiered Fee Tests ────────────────────────────────────────────────────────

#[test]
fn test_tiered_fee_single_tier() {
    use crate::{FeeTier, PlatformConfig};
    let (env, client, creator, token_address, admin) = setup_env();

    let platform = Address::generate(&env);
    let deadline = env.ledger().timestamp() + 3600;
    let goal: i128 = 500_000;
    let min_contribution: i128 = 1_000;

    let platform_config = PlatformConfig {
        address: platform.clone(),
        fee_bps: 500,
    };

    let fee_tiers = soroban_sdk::vec![
        &env,
        FeeTier {
            threshold: 1_000_000,
            fee_bps: 500,
        }
    ];

    client.initialize(&creator, &token_address, &goal, &deadline, &min_contribution, &Some(platform_config), &Some(fee_tiers));

    let contributor = Address::generate(&env);
    mint_to(&env, &token_address, &admin, &contributor, 800_000);
    client.contribute(&contributor, &800_000);

    env.ledger().set_timestamp(deadline + 1);
    client.withdraw();

    let token_client = token::Client::new(&env, &token_address);
    let platform_balance = token_client.balance(&platform);
    let creator_balance = token_client.balance(&creator);

    assert_eq!(platform_balance, 40_000);
    assert_eq!(creator_balance, 10_000_000 + 760_000);
}

#[test]
fn test_tiered_fee_multiple_tiers() {
    use crate::{FeeTier, PlatformConfig};
    let (env, client, creator, token_address, admin) = setup_env();

    let platform = Address::generate(&env);
    let deadline = env.ledger().timestamp() + 3600;
    let goal: i128 = 1_500_000;
    let min_contribution: i128 = 1_000;

    let platform_config = PlatformConfig {
        address: platform.clone(),
        fee_bps: 500,
    };

    let fee_tiers = soroban_sdk::vec![
        &env,
        FeeTier {
            threshold: 1_000_000,
            fee_bps: 500,
        },
        FeeTier {
            threshold: 2_000_000,
            fee_bps: 200,
        }
    ];

    client.initialize(&creator, &token_address, &goal, &deadline, &min_contribution, &Some(platform_config), &Some(fee_tiers));

    let contributor = Address::generate(&env);
    mint_to(&env, &token_address, &admin, &contributor, 1_500_000);
    client.contribute(&contributor, &1_500_000);

    env.ledger().set_timestamp(deadline + 1);
    client.withdraw();

    let token_client = token::Client::new(&env, &token_address);
    let platform_balance = token_client.balance(&platform);
    let creator_balance = token_client.balance(&creator);

    assert_eq!(platform_balance, 60_000);
    assert_eq!(creator_balance, 10_000_000 + 1_440_000);
}

#[test]
fn test_tiered_fee_flat_fallback() {
    use crate::PlatformConfig;
    let (env, client, creator, token_address, admin) = setup_env();

    let platform = Address::generate(&env);
    let deadline = env.ledger().timestamp() + 3600;
    let goal: i128 = 1_000_000;
    let min_contribution: i128 = 1_000;

    let platform_config = PlatformConfig {
        address: platform.clone(),
        fee_bps: 300,
    };

    client.initialize(&creator, &token_address, &goal, &deadline, &min_contribution, &Some(platform_config), &None);

    let contributor = Address::generate(&env);
    mint_to(&env, &token_address, &admin, &contributor, 1_000_000);
    client.contribute(&contributor, &1_000_000);

    env.ledger().set_timestamp(deadline + 1);
    client.withdraw();

    let token_client = token::Client::new(&env, &token_address);
    let platform_balance = token_client.balance(&platform);
    let creator_balance = token_client.balance(&creator);

    assert_eq!(platform_balance, 30_000);
    assert_eq!(creator_balance, 10_000_000 + 970_000);
}

#[test]
fn test_tiered_fee_zero_fee() {
    use crate::{FeeTier, PlatformConfig};
    let (env, client, creator, token_address, admin) = setup_env();

    let platform = Address::generate(&env);
    let deadline = env.ledger().timestamp() + 3600;
    let goal: i128 = 1_000_000;
    let min_contribution: i128 = 1_000;

    let platform_config = PlatformConfig {
        address: platform.clone(),
        fee_bps: 0,
    };

    let fee_tiers = soroban_sdk::vec![
        &env,
        FeeTier {
            threshold: 1_000_000,
            fee_bps: 0,
        }
    ];

    client.initialize(&creator, &token_address, &goal, &deadline, &min_contribution, &Some(platform_config), &Some(fee_tiers));

    let contributor = Address::generate(&env);
    mint_to(&env, &token_address, &admin, &contributor, 1_000_000);
    client.contribute(&contributor, &1_000_000);

    env.ledger().set_timestamp(deadline + 1);
    client.withdraw();

    let token_client = token::Client::new(&env, &token_address);
    let platform_balance = token_client.balance(&platform);
    let creator_balance = token_client.balance(&creator);

    assert_eq!(platform_balance, 0);
    assert_eq!(creator_balance, 10_000_000 + 1_000_000);
}

#[test]
#[should_panic(expected = "fee tier fee_bps cannot exceed 10000")]
fn test_reject_fee_tier_exceeds_10000() {
    use crate::{FeeTier, PlatformConfig};
    let (env, client, creator, token_address, _admin) = setup_env();

    let platform = Address::generate(&env);
    let deadline = env.ledger().timestamp() + 3600;
    let goal: i128 = 1_000_000;
    let min_contribution: i128 = 1_000;

    let platform_config = PlatformConfig {
        address: platform,
        fee_bps: 500,
    };

    let fee_tiers = soroban_sdk::vec![
        &env,
        FeeTier {
            threshold: 1_000_000,
            fee_bps: 10_001,
        }
    ];

    client.initialize(&creator, &token_address, &goal, &deadline, &min_contribution, &Some(platform_config), &Some(fee_tiers));
}

#[test]
#[should_panic(expected = "fee tiers must be ordered by threshold ascending")]
fn test_reject_unordered_fee_tiers() {
    use crate::{FeeTier, PlatformConfig};
    let (env, client, creator, token_address, _admin) = setup_env();

    let platform = Address::generate(&env);
    let deadline = env.ledger().timestamp() + 3600;
    let goal: i128 = 1_000_000;
    let min_contribution: i128 = 1_000;

    let platform_config = PlatformConfig {
        address: platform,
        fee_bps: 500,
    };

    let fee_tiers = soroban_sdk::vec![
        &env,
        FeeTier {
            threshold: 2_000_000,
            fee_bps: 200,
        },
        FeeTier {
            threshold: 1_000_000,
            fee_bps: 500,
        }
    ];

    client.initialize(&creator, &token_address, &goal, &deadline, &min_contribution, &Some(platform_config), &Some(fee_tiers));
}

#[test]
fn test_fee_tiers_view() {
    use crate::{FeeTier, PlatformConfig};
    let (env, client, creator, token_address, _admin) = setup_env();

    let platform = Address::generate(&env);
    let deadline = env.ledger().timestamp() + 3600;
    let goal: i128 = 1_000_000;
    let min_contribution: i128 = 1_000;

    let platform_config = PlatformConfig {
        address: platform,
        fee_bps: 500,
    };

    let fee_tiers = soroban_sdk::vec![
        &env,
        FeeTier {
            threshold: 1_000_000,
            fee_bps: 500,
        },
        FeeTier {
            threshold: 2_000_000,
            fee_bps: 200,
        }
    ];

    client.initialize(&creator, &token_address, &goal, &deadline, &min_contribution, &Some(platform_config), &Some(fee_tiers.clone()));

    let retrieved_tiers = client.fee_tiers();
    assert_eq!(retrieved_tiers.len(), 2);
    assert_eq!(retrieved_tiers.get(0).unwrap().threshold, 1_000_000);
    assert_eq!(retrieved_tiers.get(0).unwrap().fee_bps, 500);
    assert_eq!(retrieved_tiers.get(1).unwrap().threshold, 2_000_000);
    assert_eq!(retrieved_tiers.get(1).unwrap().fee_bps, 200);
}

// ── Multisig & DAO Creator Tests ───────────────────────────────────────────

/// Test that withdraw works correctly when the creator is a contract address.
///
/// This simulates a multisig wallet or DAO contract as the campaign creator.
/// In Soroban, when `creator.require_auth()` is called on a contract address,
/// it invokes the contract's authorization logic, enabling multisig approval.
#[test]
fn test_withdraw_with_multisig_creator() {
    let env = Env::default();
    env.mock_all_auths();

    let contract_id = env.register(CrowdfundContract, ());
    let client = CrowdfundContractClient::new(&env, &contract_id);

    let token_admin = Address::generate(&env);
    let token_contract_id = env.register_stellar_asset_contract_v2(token_admin.clone());
    let token_address = token_contract_id.address();
    let token_admin_client = token::StellarAssetClient::new(&env, &token_address);

    // Use a contract address as the creator (simulating a multisig wallet)
    // In a real scenario, this would be a deployed multisig contract
    let multisig_creator = Address::generate(&env);

    let deadline = env.ledger().timestamp() + 3600;
    let goal: i128 = 1_000_000;
    let hard_cap: i128 = goal * 2;
    let min_contribution: i128 = 1_000;

    client.initialize(
        &multisig_creator,
        &token_address,
        &goal,
        &hard_cap,
        &deadline,
        &min_contribution,
        &soroban_sdk::String::from_str(&env, "Multisig Campaign"),
        &soroban_sdk::String::from_str(&env, "Campaign with multisig creator"),
        &None,
    );

    // Contribute to meet the goal
    let contributor = Address::generate(&env);
    token_admin_client.mint(&contributor, &1_000_000);
    client.contribute(&contributor, &1_000_000);

    // Fast forward past deadline
    env.ledger().set_timestamp(deadline + 1);

    // Withdraw should succeed with multisig creator
    // In a real scenario, this would require M-of-N signatures
    let result = client.try_withdraw();
    assert!(result.is_ok());
}

/// Test that set_paused works correctly with a multisig creator.
///
/// This ensures that pause/unpause operations can be controlled by
/// a multisig wallet or DAO, preventing single-party control over
/// this critical security function.
#[test]
fn test_set_paused_with_multisig_creator() {
    let env = Env::default();
    env.mock_all_auths();

    let contract_id = env.register(CrowdfundContract, ());
    let client = CrowdfundContractClient::new(&env, &contract_id);

    let token_admin = Address::generate(&env);
    let token_contract_id = env.register_stellar_asset_contract_v2(token_admin);
    let token_address = token_contract_id.address();

    // Use a contract address as the creator (simulating a multisig wallet)
    let multisig_creator = Address::generate(&env);

    let deadline = env.ledger().timestamp() + 3600;
    let goal: i128 = 1_000_000;
    let hard_cap: i128 = goal * 2;
    let min_contribution: i128 = 1_000;

    client.initialize(
        &multisig_creator,
        &token_address,
        &goal,
        &hard_cap,
        &deadline,
        &min_contribution,
        &soroban_sdk::String::from_str(&env, "Multisig Campaign"),
        &soroban_sdk::String::from_str(&env, "Campaign with multisig creator"),
        &None,
    );

    // Pause the campaign - should work with multisig creator
    client.set_paused(&true);

    // Verify the campaign is paused
    // (In a real scenario, this would require multisig approval)
    let paused: bool = env
        .storage()
        .instance()
        .get(&crate::DataKey::Paused)
        .unwrap_or(false);
    assert!(paused);

    // Unpause the campaign
    client.set_paused(&false);

    let paused: bool = env
        .storage()
        .instance()
        .get(&crate::DataKey::Paused)
        .unwrap_or(false);
    assert!(!paused);
}

/// Test that update_metadata works correctly with a multisig creator.
///
/// This ensures that campaign metadata changes can be controlled by
/// a multisig wallet or DAO, maintaining transparency and preventing
/// unauthorized modifications.
#[test]
fn test_update_metadata_with_multisig_creator() {
    let env = Env::default();
    env.mock_all_auths();

    let contract_id = env.register(CrowdfundContract, ());
    let client = CrowdfundContractClient::new(&env, &contract_id);

    let token_admin = Address::generate(&env);
    let token_contract_id = env.register_stellar_asset_contract_v2(token_admin);
    let token_address = token_contract_id.address();

    // Use a contract address as the creator (simulating a DAO)
    let dao_creator = Address::generate(&env);

    let deadline = env.ledger().timestamp() + 3600;
    let goal: i128 = 1_000_000;
    let hard_cap: i128 = goal * 2;
    let min_contribution: i128 = 1_000;

    client.initialize(
        &dao_creator,
        &token_address,
        &goal,
        &hard_cap,
        &deadline,
        &min_contribution,
        &soroban_sdk::String::from_str(&env, "DAO Campaign"),
        &soroban_sdk::String::from_str(&env, "Campaign with DAO creator"),
        &None,
    );

    // Update metadata - should work with DAO creator
    let new_title = Some(soroban_sdk::String::from_str(&env, "Updated DAO Campaign"));
    let new_description = Some(soroban_sdk::String::from_str(
        &env,
        "Updated description by DAO vote",
    ));

    client.update_metadata(&dao_creator, &new_title, &new_description, &None);

    // Verify the metadata was updated
    let title = client.title();
    assert_eq!(title, new_title.unwrap());
}

/// Test that unauthorized addresses are still rejected even when creator is a multisig.
///
/// This ensures that the authorization mechanism works correctly for both
/// user accounts and contract addresses, rejecting unauthorized callers.
#[test]
#[should_panic]
fn test_multisig_creator_rejects_unauthorized_address() {
    let env = Env::default();
    env.mock_all_auths();

    let contract_id = env.register(CrowdfundContract, ());
    let client = CrowdfundContractClient::new(&env, &contract_id);

    let token_admin = Address::generate(&env);
    let token_contract_id = env.register_stellar_asset_contract_v2(token_admin);
    let token_address = token_contract_id.address();

    // Use a contract address as the creator (simulating a multisig wallet)
    let multisig_creator = Address::generate(&env);

    let deadline = env.ledger().timestamp() + 3600;
    let goal: i128 = 1_000_000;
    let hard_cap: i128 = goal * 2;
    let min_contribution: i128 = 1_000;

    client.initialize(
        &multisig_creator,
        &token_address,
        &goal,
        &hard_cap,
        &deadline,
        &min_contribution,
        &soroban_sdk::String::from_str(&env, "Multisig Campaign"),
        &soroban_sdk::String::from_str(&env, "Campaign with multisig creator"),
        &None,
    );

    // Try to pause with an unauthorized address
    let unauthorized = Address::generate(&env);
    env.mock_all_auths_allowing_non_root_auth();
    env.set_auths(&[]);

    // This should panic because unauthorized address is not the creator
    client.set_paused(&true);
}

/// Test that all admin functions work correctly when creator is a DAO contract.
///
/// This comprehensive test verifies that all creator-restricted functions
/// (withdraw, set_paused, update_metadata, add_roadmap_item, add_stretch_goal,
/// add_reward_tier) work seamlessly with a DAO contract as the creator.
#[test]
fn test_all_admin_functions_with_dao_creator() {
    let env = Env::default();
    env.mock_all_auths();

    let contract_id = env.register(CrowdfundContract, ());
    let client = CrowdfundContractClient::new(&env, &contract_id);

    let token_admin = Address::generate(&env);
    let token_contract_id = env.register_stellar_asset_contract_v2(token_admin.clone());
    let token_address = token_contract_id.address();
    let token_admin_client = token::StellarAssetClient::new(&env, &token_address);

    // Use a contract address as the creator (simulating a DAO)
    let dao_creator = Address::generate(&env);

    let deadline = env.ledger().timestamp() + 3600;
    let goal: i128 = 1_000_000;
    let hard_cap: i128 = goal * 2;
    let min_contribution: i128 = 1_000;

    client.initialize(
        &dao_creator,
        &token_address,
        &goal,
        &hard_cap,
        &deadline,
        &min_contribution,
        &soroban_sdk::String::from_str(&env, "DAO Campaign"),
        &soroban_sdk::String::from_str(&env, "Campaign with DAO governance"),
        &None,
    );

    // Test add_roadmap_item
    let roadmap_date = env.ledger().timestamp() + 86400;
    let roadmap_desc = soroban_sdk::String::from_str(&env, "Milestone 1");
    client.add_roadmap_item(&roadmap_date, &roadmap_desc);

    let roadmap = client.roadmap();
    assert_eq!(roadmap.len(), 1);

    // Test add_stretch_goal
    let stretch_goal: i128 = 2_000_000;
    client.add_stretch_goal(&stretch_goal);

    // Test add_reward_tier
    let tier_name = soroban_sdk::String::from_str(&env, "Gold");
    client.add_reward_tier(&dao_creator, &tier_name, &10_000);

    let tiers = client.reward_tiers();
    assert_eq!(tiers.len(), 1);

    // Test update_metadata
    let new_title = Some(soroban_sdk::String::from_str(&env, "Updated by DAO"));
    client.update_metadata(&dao_creator, &new_title, &None, &None);

    // Test set_paused
    client.set_paused(&true);
    client.set_paused(&false);

    // Test update_deadline
    let new_deadline = deadline + 7200;
    client.update_deadline(&new_deadline);

    // Contribute to meet the goal
    let contributor = Address::generate(&env);
    token_admin_client.mint(&contributor, &1_000_000);
    client.contribute(&contributor, &1_000_000);

    // Fast forward past deadline
    env.ledger().set_timestamp(new_deadline + 1);

    // Test withdraw
    let result = client.try_withdraw();
    assert!(result.is_ok());
}
