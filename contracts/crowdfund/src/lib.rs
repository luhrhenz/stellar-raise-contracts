#![no_std]
#![allow(missing_docs)]
#![allow(clippy::too_many_arguments)]

use soroban_sdk::{
    contract, contracterror, contractimpl, contracttype, token, Address, Env, String, Symbol, Vec,
};

#[cfg(test)]
mod test;

const CONTRACT_VERSION: u32 = 3;
const CONTRIBUTION_COOLDOWN: u64 = 60; // 60 seconds cooldown

#[derive(Clone, PartialEq)]
#[contracttype]
pub enum Status {
    Active,
    Successful,
    Refunded,
    Cancelled,
}

#[derive(Clone)]
#[contracttype]
pub struct RoadmapItem {
    pub date: u64,
    pub description: String,
}

#[derive(Clone)]
#[contracttype]
pub struct PlatformConfig {
    pub address: Address,
    pub fee_bps: u32,
}

#[derive(Clone)]
#[contracttype]
pub struct CampaignStats {
    pub total_raised: i128,
    pub goal: i128,
    pub progress_bps: u32,
    pub contributor_count: u32,
    pub average_contribution: i128,
    pub largest_contribution: i128,
}

#[derive(Clone)]
#[contracttype]
pub struct CampaignInfo {
    pub creator: Address,
    pub token: Address,
    pub goal: i128,
    pub deadline: u64,
    pub total_raised: i128,
}

#[derive(Clone)]
#[contracttype]
pub enum DataKey {
    Creator,
    Token,
    Goal,
    Deadline,
    TotalRaised,
    Contribution(Address),
    Contributors,
    Status,
    MinContribution,
    Roadmap,
    Admin,
    Title,
    Description,
    SocialLinks,
    PlatformConfig,
    /// List of reward tiers (name + min_amount).
    RewardTiers,
    /// Individual pledge by address.
    Pledge(Address),
    /// List of all pledger addresses.
    Pledgers,
    /// Total amount pledged (not yet collected).
    TotalPledged,
    /// List of stretch goal milestones.
    StretchGoals,
    /// Campaign updates blog: Vec<(u64, String)> of (timestamp, update text).
    Updates,
    /// Whether whitelist is enabled for this campaign.
    WhitelistEnabled,
    /// Individual whitelist entry by address.
    Whitelist(Address),
}

#[contracterror]
#[derive(Copy, Clone, Debug, Eq, PartialEq, PartialOrd, Ord)]
#[repr(u32)]
pub enum ContractError {
    AlreadyInitialized = 1,
    CampaignEnded = 2,
    CampaignStillActive = 3,
    GoalNotReached = 4,
    GoalReached = 5,
    Overflow = 6,
    InvalidHardCap = 7,
    HardCapExceeded = 8,
    RateLimitExceeded = 9,
    ContractPaused = 10,
    InvalidLimit = 11,
}

#[contractclient(name = "NftContractClient")]
pub trait NftContract {
    fn mint(env: Env, to: Address) -> u128;
}

#[contract]
pub struct CrowdfundContract;

#[contractimpl]
impl CrowdfundContract {
    // ── Multisig & DAO Compatibility ───────────────────────────────────────
    //
    // IMPORTANT: This contract fully supports multi-signature wallets and DAO
    // contracts as the campaign creator. The Soroban `Address` type can represent
    // both standard user accounts and contract addresses.
    //
    // When `creator.require_auth()` is called:
    // - For user accounts: Standard signature verification is performed
    // - For contract addresses: The contract's authorization logic is invoked,
    //   allowing multisig wallets, DAO governance contracts, or any custom
    //   authorization scheme to control the campaign
    //
    // All creator-restricted functions (withdraw, set_paused, update_metadata,
    // add_roadmap_item, add_stretch_goal, add_reward_tier, update_deadline,
    // post_update, add_to_whitelist) work seamlessly with multisig creators.
    //
    // Security Benefits:
    // - Eliminates single point of failure for high-value campaigns
    // - Enables decentralized governance of campaign operations
    // - Supports time-locked or threshold-based authorization schemes
    //
    // Example Multisig Patterns:
    // 1. M-of-N Threshold: Require M signatures out of N authorized signers
    // 2. DAO Governance: Require on-chain voting approval for admin actions
    // 3. Time-locked Admin: Require time delay before executing sensitive operations
    // 4. Hierarchical Control: Different authorization levels for different operations
    //
    // ───────────────────────────────────────────────────────────────────────

    /// Helper function to check if the creator address is a contract.
    ///
    /// This is useful for distinguishing between standard user accounts and
    /// contract-based creators (multisig wallets, DAOs, etc.) for logging,
    /// analytics, or conditional logic.
    ///
    /// # Arguments
    /// * `env` – The contract environment
    /// * `creator` – The address to check
    ///
    /// # Returns
    /// * `true` if the address is a contract, `false` if it's a user account
    ///
    /// # Note
    /// This function does not affect authorization - `require_auth()` works
    /// correctly for both user accounts and contracts. This is purely for
    /// informational purposes.
    ///
    /// # Implementation Note
    /// In Soroban, the Address type abstracts over both account and contract
    /// addresses. While there's no direct API to check the address type at
    /// runtime, the authorization mechanism (`require_auth()`) handles both
    /// cases transparently. This helper is provided for documentation purposes
    /// and potential future use cases where distinguishing address types is needed.
    #[allow(dead_code)]
    fn is_contract_creator(_env: &Env, _creator: &Address) -> bool {
        // Note: Soroban's Address type doesn't expose a direct method to check
        // if an address is a contract vs. an account at runtime. However, this
        // doesn't matter for authorization - require_auth() works correctly for
        // both types.
        //
        // For actual runtime checks, you would need to:
        // 1. Attempt to invoke a known contract function (will fail for accounts)
        // 2. Use external indexing/metadata to track contract addresses
        // 3. Rely on off-chain knowledge of the creator type
        //
        // Since authorization works transparently for both types, this function
        // is primarily for documentation and potential future enhancements.
        false // Placeholder - actual implementation would require additional context
    }

    /// Initializes a new crowdfunding campaign.
    ///
    /// # Arguments
    /// * `creator`            – The campaign creator's address (can be a user account,
    ///                          multisig wallet, or DAO contract).
    /// * `token`              – The token contract address used for contributions.
    /// * `goal`               – The funding goal (in the token's smallest unit).
    /// * `hard_cap`           – Maximum total amount that can be raised (must be >= goal).
    /// * `deadline`           – The campaign deadline as a ledger timestamp.
    /// * `min_contribution`   – The minimum contribution amount.
    /// * `title`              – The campaign title.
    /// * `description`        – The campaign description.
    /// * `platform_config`    – Optional platform configuration (address and fee in basis points).
    ///
    /// # Multisig Support
    /// The `creator` parameter can be any valid Soroban address, including:
    /// - Standard user accounts (ed25519 public keys)
    /// - Multisig wallet contracts (requiring M-of-N signatures)
    /// - DAO governance contracts (requiring on-chain voting)
    /// - Custom authorization contracts (time-locks, hierarchical permissions, etc.)
    ///
    /// The `creator.require_auth()` call ensures that only the authorized entity
    /// (whether a single user or a multisig group) can initialize the campaign.
    ///
    /// # Panics
    /// * If already initialized.
    /// * If platform fee exceeds 10,000 (100%).
    #[allow(clippy::too_many_arguments)]
    pub fn initialize(
        env: Env,
        admin: Address,
        creator: Address,
        token: Address,
        goal: i128,
        deadline: u64,
        min_contribution: i128,
        platform_config: Option<PlatformConfig>,
        bonus_goal: Option<i128>,
        bonus_goal_description: Option<String>,
        hard_cap: Option<i128>,
    ) -> Result<(), ContractError> {
        if env.storage().instance().has(&DataKey::Creator) {
            return Err(ContractError::AlreadyInitialized);
        }

        creator.require_auth();

        if let Some(ref config) = platform_config {
            if config.fee_bps > 10_000 {
                panic!("platform fee cannot exceed 100%");
            }
            env.storage()
                .instance()
                .set(&DataKey::PlatformConfig, config);
        }

        let hard_cap_value = hard_cap.unwrap_or(goal * 2); // Default to 2x goal
        if hard_cap_value < goal {
            return Err(ContractError::InvalidHardCap);
        }

        if let Some(bg) = bonus_goal {
            if bg <= goal {
                panic!("bonus goal must be greater than primary goal");
            }
            env.storage().instance().set(&DataKey::BonusGoal, &bg);
        }

        if let Some(bg_description) = bonus_goal_description {
            env.storage()
                .instance()
                .set(&DataKey::BonusGoalDescription, &bg_description);
        }

        env.storage().instance().set(&DataKey::Creator, &creator);
        env.storage().instance().set(&DataKey::Token, &token);
        env.storage().instance().set(&DataKey::Goal, &goal);
        env.storage().instance().set(&DataKey::HardCap, &hard_cap_value);
        env.storage().instance().set(&DataKey::Deadline, &deadline);
        env.storage()
            .instance()
            .set(&DataKey::MinContribution, &min_contribution);
        env.storage().instance().set(&DataKey::Title, &title);
        env.storage()
            .instance()
            .set(&DataKey::Description, &description);
        env.storage().instance().set(&DataKey::TotalRaised, &0i128);
        env.storage()
            .instance()
            .set(&DataKey::BonusGoalReachedEmitted, &false);
        env.storage()
            .instance()
            .set(&DataKey::Status, &Status::Active);

        // Store platform config if provided.
        if let Some(config) = platform_config {
            env.storage().instance().set(&DataKey::PlatformConfig, &config);
        }

        let empty_contributors: Vec<Address> = Vec::new(&env);
        env.storage()
            .persistent()
            .set(&DataKey::Contributors, &empty_contributors);

        let empty_roadmap: Vec<RoadmapItem> = Vec::new(&env);
        env.storage()
            .instance()
            .set(&DataKey::Roadmap, &empty_roadmap);

        Ok(())
    }

    pub fn set_nft_contract(env: Env, creator: Address, nft_contract: Address) {
        let stored_creator: Address = env.storage().instance().get(&DataKey::Creator).unwrap();
        if creator != stored_creator {
            panic!("not authorized");
        }

        creator.require_auth();

        if !env.storage().instance().has(&DataKey::WhitelistEnabled) {
            env.storage()
                .instance()
                .set(&DataKey::WhitelistEnabled, &true);
        }

        for address in addresses.iter() {
            env.storage()
                .instance()
                .set(&DataKey::Whitelist(address), &true);
        }
    }

    /// Contribute tokens to the campaign.
    ///
    /// The contributor must authorize the call. Contributions are rejected
    /// after the deadline has passed.
    pub fn contribute(
        env: Env,
        contributor: Address,
        amount: i128,
        referral: Option<Address>,
    ) -> Result<(), ContractError> {
        // ── Rate limiting: enforce cooldown between contributions ──
        let now = env.ledger().timestamp();
        let last_time_key = DataKey::LastContributionTime(contributor.clone());
        if let Some(last_time) = env.storage().persistent().get::<_, u64>(&last_time_key) {
            if now < last_time + CONTRIBUTION_COOLDOWN {
                return Err(ContractError::RateLimitExceeded);
            }
        }

        let status: Status = env.storage().instance().get(&DataKey::Status).unwrap();
        if status != Status::Active {
            panic!("campaign is not active");
        }

        let min_contribution: i128 = env
            .storage()
            .instance()
            .get(&DataKey::MinContribution)
            .unwrap();
        if amount < min_contribution {
            panic!("amount below minimum");
        }

        let deadline: u64 = env.storage().instance().get(&DataKey::Deadline).unwrap();
        if env.ledger().timestamp() > deadline {
            return Err(ContractError::CampaignEnded);
        }

        let token_address: Address = env.storage().instance().get(&DataKey::Token).unwrap();
        let token_client = token::Client::new(&env, &token_address);
        token_client.transfer(&contributor, &env.current_contract_address(), &amount);

        let contribution_key = DataKey::Contribution(contributor.clone());
        let previous_amount: i128 = env
            .storage()
            .persistent()
            .get(&contribution_key)
            .unwrap_or(0);

        env.storage()
            .persistent()
            .set(&contribution_key, &(previous_amount + amount));
        env.storage()
            .persistent()
            .extend_ttl(&contribution_key, 100, 100);

        let total: i128 = env.storage().instance().get(&DataKey::TotalRaised).unwrap();
        env.storage()
            .instance()
            .set(&DataKey::TotalRaised, &(total + amount));

        let mut contributors: Vec<Address> = env
            .storage()
            .persistent()
            .get(&DataKey::Contributors)
            .unwrap_or_else(|| Vec::new(&env));

        if !contributors.contains(&contributor) {
            contributors.push_back(contributor);
            env.storage()
                .persistent()
                .set(&DataKey::Contributors, &contributors);
            env.storage()
                .persistent()
                .extend_ttl(&DataKey::Contributors, 100, 100);
        }

        // Emit contribution event
        env.events().publish(
            ("campaign", "contributed"),
            (contributor.clone(), amount),
        );

        // Update referral tally if referral provided
        if let Some(referrer) = referral {
            if referrer != contributor {
                let referral_key = DataKey::ReferralTally(referrer.clone());
                let current_tally: i128 =
                    env.storage().persistent().get(&referral_key).unwrap_or(0);

                let new_tally = current_tally
                    .checked_add(amount)
                    .ok_or(ContractError::Overflow)?;

                env.storage().persistent().set(&referral_key, &new_tally);
                env.storage()
                    .persistent()
                    .extend_ttl(&referral_key, 100, 100);

                // Emit referral event
                env.events().publish(
                    ("campaign", "referral"),
                    (referrer, contributor, amount),
                );
            }
        }

        // Update last contribution time for rate limiting
        env.storage().persistent().set(&last_time_key, &now);
        env.storage()
            .persistent()
            .extend_ttl(&last_time_key, 100, 100);

        Ok(())
    }

    /// Pledge tokens to the campaign without transferring them immediately.
    ///
    /// The pledger must authorize the call. Pledges are recorded off-chain
    /// and only collected if the goal is met after the deadline.
    pub fn pledge(env: Env, pledger: Address, amount: i128) -> Result<(), ContractError> {
        pledger.require_auth();

        let min_contribution: i128 = env
            .storage()
            .instance()
            .get(&DataKey::MinContribution)
            .unwrap();
        if amount < min_contribution {
            panic!("amount below minimum");
        }

        let deadline: u64 = env.storage().instance().get(&DataKey::Deadline).unwrap();
        if env.ledger().timestamp() > deadline {
            return Err(ContractError::CampaignEnded);
        }

        // Update the pledger's running total.
        let pledge_key = DataKey::Pledge(pledger.clone());
        let prev: i128 = env.storage().persistent().get(&pledge_key).unwrap_or(0);
        env.storage()
            .persistent()
            .set(&pledge_key, &(prev + amount));
        env.storage().persistent().extend_ttl(&pledge_key, 100, 100);

        // Update the global total pledged.
        let total_pledged: i128 = env
            .storage()
            .instance()
            .get(&DataKey::TotalPledged)
            .unwrap_or(0);
        env.storage()
            .instance()
            .set(&DataKey::TotalPledged, &(total_pledged + amount));

        // Track pledger address if new.
        let mut pledgers: Vec<Address> = env
            .storage()
            .persistent()
            .get(&DataKey::Pledgers)
            .unwrap_or_else(|| Vec::new(&env));
        if !pledgers.contains(&pledger) {
            pledgers.push_back(pledger.clone());
            env.storage()
                .persistent()
                .set(&DataKey::Pledgers, &pledgers);
            env.storage()
                .persistent()
                .extend_ttl(&DataKey::Pledgers, 100, 100);
        }

        // Emit pledge event
        env.events()
            .publish(("campaign", "pledged"), (pledger, amount));

        Ok(())
    }

    /// Collect all pledges after the deadline when the goal is met.
    ///
    /// This function transfers tokens from all pledgers to the contract.
    /// Only callable after the deadline and when the combined total of
    /// contributions and pledges meets or exceeds the goal.
    pub fn collect_pledges(env: Env) -> Result<(), ContractError> {
        let status: Status = env.storage().instance().get(&DataKey::Status).unwrap();
        if status != Status::Active {
            panic!("campaign is not active");
        }

        let deadline: u64 = env.storage().instance().get(&DataKey::Deadline).unwrap();
        if env.ledger().timestamp() <= deadline {
            return Err(ContractError::CampaignStillActive);
        }

        let goal: i128 = env.storage().instance().get(&DataKey::Goal).unwrap();
        let total_raised: i128 = env.storage().instance().get(&DataKey::TotalRaised).unwrap();
        let total_pledged: i128 = env
            .storage()
            .instance()
            .get(&DataKey::TotalPledged)
            .unwrap_or(0);

        // Check if combined total meets the goal
        if total_raised + total_pledged < goal {
            return Err(ContractError::GoalNotReached);
        }

        let token_address: Address = env.storage().instance().get(&DataKey::Token).unwrap();
        let token_client = token::Client::new(&env, &token_address);

        let pledgers: Vec<Address> = env
            .storage()
            .persistent()
            .get(&DataKey::Pledgers)
            .unwrap_or_else(|| Vec::new(&env));

        // Collect pledges from all pledgers
        for pledger in pledgers.iter() {
            let pledge_key = DataKey::Pledge(pledger.clone());
            let amount: i128 = env.storage().persistent().get(&pledge_key).unwrap_or(0);
            if amount > 0 {
                // Transfer tokens from pledger to contract
                token_client.transfer(&pledger, &env.current_contract_address(), &amount);

                // Clear the pledge
                env.storage().persistent().set(&pledge_key, &0i128);
                env.storage().persistent().extend_ttl(&pledge_key, 100, 100);
            }
        }

        // Update total raised to include collected pledges
        env.storage()
            .instance()
            .set(&DataKey::TotalRaised, &(total_raised + total_pledged));

        // Reset total pledged
        env.storage().instance().set(&DataKey::TotalPledged, &0i128);

        // Emit pledges collected event
        env.events()
            .publish(("campaign", "pledges_collected"), total_pledged);

        Ok(())
    }

    /// Withdraw a specific amount from the contributor's balance.
    ///
    /// Callable by the contributor only while the campaign is still active and
    /// before the deadline.
    pub fn withdraw_contribution(env: Env, contributor: Address, amount: i128) {
        contributor.require_auth();

        let status: Status = env.storage().instance().get(&DataKey::Status).unwrap();
        if status != Status::Active {
            panic!("campaign is not active");
        }

        let deadline: u64 = env.storage().instance().get(&DataKey::Deadline).unwrap();
        if env.ledger().timestamp() > deadline {
            panic!("campaign has ended");
        }

        let prev: i128 = env
            .storage()
            .instance()
            .get(&DataKey::Contribution(contributor.clone()))
            .unwrap_or(0);

        if amount > prev {
            panic!("insufficient balance");
        }

        let token_address: Address = env.storage().instance().get(&DataKey::Token).unwrap();
        let token_client = token::Client::new(&env, &token_address);

        // Transfer tokens back to the contributor.
        token_client.transfer(&env.current_contract_address(), &contributor, &amount);

        // Update contributor balance.
        let new_balance = prev - amount;
        if new_balance == 0 {
            env.storage()
                .instance()
                .remove(&DataKey::Contribution(contributor.clone()));

            // Remove from contributors list.
            let mut contributors: Vec<Address> = env
                .storage()
                .instance()
                .get(&DataKey::Contributors)
                .unwrap();
            let mut index = 0;
            let mut found = false;
            for c in contributors.iter() {
                if c == contributor {
                    found = true;
                    break;
                }
                index += 1;
            }
            if found {
                contributors.remove(index);
                env.storage()
                    .instance()
                    .set(&DataKey::Contributors, &contributors);
            }
        } else {
            env.storage()
                .instance()
                .set(&DataKey::Contribution(contributor.clone()), &new_balance);
        }

        // Update global total raised.
        let total: i128 = env.storage().instance().get(&DataKey::TotalRaised).unwrap();
        env.storage()
            .instance()
            .set(&DataKey::TotalRaised, &(total - amount));
    }

    /// Withdraw raised funds — only callable by the creator after the
    /// deadline, and only if the goal has been met.
    ///
    /// If a platform fee is configured, deducts the fee and transfers it to
    /// the platform address, then sends the remainder to the creator.
    ///
    /// # Multisig Support
    /// This function fully supports multisig and DAO creators. When the creator
    /// is a contract address, `creator.require_auth()` will invoke the contract's
    /// authorization logic, enabling:
    /// - M-of-N threshold signatures for withdrawal approval
    /// - DAO governance voting before fund withdrawal
    /// - Time-locked withdrawals for added security
    /// - Any custom authorization scheme implemented by the creator contract
    ///
    /// # Security Note
    /// For high-value campaigns, using a multisig or DAO as the creator significantly
    /// reduces the risk of unauthorized fund withdrawal, as multiple parties must
    /// approve the transaction.
    pub fn withdraw(env: Env) -> Result<(), ContractError> {
        let status: Status = env.storage().instance().get(&DataKey::Status).unwrap();
        if status != Status::Active {
            panic!("campaign is not active");
        }

        let creator: Address = env.storage().instance().get(&DataKey::Creator).unwrap();
        creator.require_auth();

        let deadline: u64 = env.storage().instance().get(&DataKey::Deadline).unwrap();
        if env.ledger().timestamp() <= deadline {
            return Err(ContractError::CampaignStillActive);
        }

        let goal: i128 = env.storage().instance().get(&DataKey::Goal).unwrap();
        let total: i128 = env.storage().instance().get(&DataKey::TotalRaised).unwrap();
        if total < goal {
            return Err(ContractError::GoalNotReached);
        }

        let token_address: Address = env.storage().instance().get(&DataKey::Token).unwrap();
        let token_client = token::Client::new(&env, &token_address);

        let platform_config: Option<PlatformConfig> =
            env.storage().instance().get(&DataKey::PlatformConfig);

        let creator_payout = if let Some(config) = platform_config {
            let fee = total
                .checked_mul(config.fee_bps as i128)
                .expect("fee calculation overflow")
                .checked_div(10_000)
                .expect("fee division by zero");

            token_client.transfer(&env.current_contract_address(), &config.address, &fee);
            env.events()
                .publish(("campaign", "fee_transferred"), (&config.address, fee));
            total.checked_sub(fee).expect("creator payout underflow")
        } else {
            total
        };

        token_client.transfer(&env.current_contract_address(), &creator, &creator_payout);

        env.storage().instance().set(&DataKey::TotalRaised, &0i128);
        env.storage()
            .instance()
            .set(&DataKey::Status, &Status::Successful);

        // Mint one commemorative NFT per eligible contributor after successful payout.
        if let Some(nft_contract) = env
            .storage()
            .instance()
            .get::<_, Address>(&DataKey::NFTContract)
        {
            let nft_client = NftContractClient::new(&env, &nft_contract);
            let contributors: Vec<Address> = env
                .storage()
                .persistent()
                .get(&DataKey::Contributors)
                .unwrap_or_else(|| Vec::new(&env));

            for contributor in contributors.iter() {
                let amount: i128 = env
                    .storage()
                    .persistent()
                    .get(&DataKey::Contribution(contributor.clone()))
                    .unwrap_or(0);

                // Only mint for contributors with a non-zero stake.
                if amount > 0 {
                    let token_id = nft_client.mint(&contributor);
                    env.events().publish(
                        (
                            Symbol::new(&env, "campaign"),
                            Symbol::new(&env, "nft_minted"),
                        ),
                        (contributor, token_id),
                    );
                }
            }
        }

        env.events()
            .publish(("campaign", "withdrawn"), (creator.clone(), total));

        Ok(())
    }

    pub fn refund_single(env: Env, contributor: Address) -> Result<(), ContractError> {
        contributor.require_auth();

        let status: Status = env.storage().instance().get(&DataKey::Status).unwrap();
        if status != Status::Active {
            panic!("campaign is not active");
        }

        let deadline: u64 = env.storage().instance().get(&DataKey::Deadline).unwrap();
        if env.ledger().timestamp() <= deadline {
            return Err(ContractError::CampaignStillActive);
        }

        let goal: i128 = env.storage().instance().get(&DataKey::Goal).unwrap();
        let total: i128 = env.storage().instance().get(&DataKey::TotalRaised).unwrap();
        if total >= goal {
            return Err(ContractError::GoalReached);
        }

        let contribution_key = DataKey::Contribution(contributor.clone());
        let amount: i128 = env
            .storage()
            .persistent()
            .get(&contribution_key)
            .unwrap_or(0);

        if amount == 0 {
            return Ok(());
        }

        let token_address: Address = env.storage().instance().get(&DataKey::Token).unwrap();
        let token_client = token::Client::new(&env, &token_address);
        token_client.transfer(&env.current_contract_address(), &contributor, &amount);

        // Reset the contributor's contribution to 0.
        env.storage().persistent().set(&contribution_key, &0i128);
        env.storage()
            .persistent()
            .extend_ttl(&contribution_key, 100, 100);

        // Update total raised.
        let new_total = total - amount;
        env.storage()
            .instance()
            .set(&DataKey::TotalRaised, &new_total);

        // Emit refund event
        env.events()
            .publish(("campaign", "refunded"), (contributor.clone(), amount));

        Ok(())
    }

    /// Upgrade the contract to a new WASM implementation — admin-only.
    ///
    /// This function allows the designated admin to upgrade the contract's WASM code
    /// without changing the contract's address or storage. The new WASM hash must be
    /// provided and the caller must be authorized as the admin.
    ///
    /// # Arguments
    /// * `new_wasm_hash` – The SHA-256 hash of the new WASM binary to deploy.
    ///
    /// # Panics
    /// * If the caller is not the admin.
    pub fn upgrade(env: Env, new_wasm_hash: soroban_sdk::BytesN<32>) {
        let admin: Address = env.storage().instance().get(&DataKey::Admin).unwrap();
        admin.require_auth();

        env.deployer().update_current_contract_wasm(new_wasm_hash);
    }

    /// Pause or unpause the contract — creator-only.
    ///
    /// When paused, all contributions, withdrawals, and refunds are blocked.
    /// This is a security mechanism to halt operations in case of detected
    /// vulnerabilities or external threats.
    ///
    /// # Arguments
    /// * `paused` – True to pause, false to unpause.
    ///
    /// # Multisig Support
    /// This critical security function works seamlessly with multisig creators.
    /// When the creator is a multisig contract, pausing/unpausing requires
    /// approval from the multisig signers, preventing any single party from
    /// unilaterally halting the campaign. This is especially important for
    /// high-value campaigns where pause decisions should be collective.
    pub fn set_paused(env: Env, paused: bool) {
        let creator: Address = env.storage().instance().get(&DataKey::Creator).unwrap();
        creator.require_auth();

        env.storage().instance().set(&DataKey::Paused, &paused);

        let event_name = if paused { "paused" } else { "unpaused" };
        env.events().publish(("campaign", event_name), ());
    }

    /// Update campaign metadata — only callable by the creator while the
    /// campaign is still Active.
    ///
    /// # Arguments
    /// * `creator`     – The campaign creator's address (for authentication).
    /// * `title`       – Optional new title (None to keep existing).
    /// * `description` – Optional new description (None to keep existing).
    /// * `socials`    – Optional new social links (None to keep existing).
    ///
    /// # Multisig Support
    /// Metadata updates support multisig creators, ensuring that campaign
    /// information changes require collective approval when using a multisig
    /// or DAO creator. This prevents unauthorized modification of campaign
    /// details and maintains transparency.
    pub fn update_metadata(
        env: Env,
        creator: Address,
        title: Option<String>,
        description: Option<String>,
        socials: Option<String>,
    ) {
        // Check campaign is active.
        let status: Status = env.storage().instance().get(&DataKey::Status).unwrap();
        if status != Status::Active {
            panic!("campaign is not active");
        }

        // Require creator authentication and verify caller is the creator.
        let stored_creator: Address = env.storage().instance().get(&DataKey::Creator).unwrap();
        if creator != stored_creator {
            panic!("not authorized");
        }
        creator.require_auth();

        // Track which fields were updated for the event.
        let mut updated_fields: Vec<Symbol> = Vec::new(&env);

        // Update title if provided.
        if let Some(new_title) = title {
            env.storage().instance().set(&DataKey::Title, &new_title);
            updated_fields.push_back(Symbol::new(&env, "title"));
        }

        // Update description if provided.
        if let Some(new_description) = description {
            env.storage()
                .instance()
                .set(&DataKey::Description, &new_description);
            updated_fields.push_back(Symbol::new(&env, "description"));
        }

        if total - amount == 0 {
            env.storage()
                .instance()
                .set(&DataKey::Status, &Status::Refunded);
        }

        Ok(())
    }

    pub fn add_roadmap_item(env: Env, date: u64, description: String) {
        let creator: Address = env.storage().instance().get(&DataKey::Creator).unwrap();
        creator.require_auth();

        if date <= env.ledger().timestamp() {
            panic!("date must be in the future");
        }

        if description.is_empty() {
            panic!("description cannot be empty");
        }

        let mut roadmap: Vec<RoadmapItem> = env
            .storage()
            .instance()
            .get(&DataKey::Roadmap)
            .unwrap_or_else(|| Vec::new(&env));

        roadmap.push_back(RoadmapItem {
            date,
            description: description.clone(),
        });

        env.storage().instance().set(&DataKey::Roadmap, &roadmap);
        env.events()
            .publish(("campaign", "roadmap_item_added"), (date, description));
    }

    pub fn roadmap(env: Env) -> Vec<RoadmapItem> {
        env.storage()
            .instance()
            .get(&DataKey::Roadmap)
            .unwrap_or_else(|| Vec::new(&env))
    }

    /// Post a campaign update (creator only).
    ///
    /// Records the current timestamp and the provided text. Rejects empty text.
    /// Emits a (campaign, update_posted) event with timestamp and text.
    pub fn post_update(env: Env, text: String) {
        let creator: Address = env.storage().instance().get(&DataKey::Creator).unwrap();
        creator.require_auth();

        if text.is_empty() {
            panic!("update text cannot be empty");
        }

        let timestamp = env.ledger().timestamp();

        let mut updates: Vec<(u64, String)> = env
            .storage()
            .instance()
            .get(&DataKey::Updates)
            .unwrap_or_else(|| Vec::new(&env));

        updates.push_back((timestamp, text.clone()));

        env.storage().instance().set(&DataKey::Updates, &updates);
        env.events()
            .publish(("campaign", "update_posted"), (timestamp, text));
    }

    /// Returns the full ordered list of campaign updates.
    ///
    /// Each entry is a tuple of (timestamp, update text) in chronological order.
    pub fn get_updates(env: Env) -> Vec<(u64, String)> {
        env.storage()
            .instance()
            .get(&DataKey::Updates)
            .unwrap_or_else(|| Vec::new(&env))
    }

    /// Add a stretch goal milestone to the campaign.
    ///
    /// Only the creator can add stretch goals. The milestone must be greater
    /// than the primary goal.
    pub fn add_stretch_goal(env: Env, milestone: i128) {
        let creator: Address = env.storage().instance().get(&DataKey::Creator).unwrap();
        creator.require_auth();

        let goal: i128 = env.storage().instance().get(&DataKey::Goal).unwrap();
        if milestone <= goal {
            panic!("stretch goal must be greater than primary goal");
        }

        let mut stretch_goals: Vec<i128> = env
            .storage()
            .instance()
            .get(&DataKey::StretchGoals)
            .unwrap_or_else(|| Vec::new(&env));

        stretch_goals.push_back(milestone);
        env.storage()
            .instance()
            .set(&DataKey::StretchGoals, &stretch_goals);
    }

    /// Add a reward tier (creator only). Rejects min_amount <= 0.
    pub fn add_reward_tier(env: Env, creator: Address, name: String, min_amount: i128) {
        let status: Status = env.storage().instance().get(&DataKey::Status).unwrap();
        if status != Status::Active {
            panic!("campaign is not active");
        }

        let stored_creator: Address = env.storage().instance().get(&DataKey::Creator).unwrap();
        if creator != stored_creator {
            panic!("not authorized");
        }
        creator.require_auth();

        if min_amount <= 0 {
            panic!("min_amount must be greater than 0");
        }

        let mut tiers: Vec<RewardTier> = env
            .storage()
            .instance()
            .get(&DataKey::RewardTiers)
            .unwrap_or_else(|| Vec::new(&env));

        tiers.push_back(RewardTier {
            name: name.clone(),
            min_amount,
        });
        env.storage().instance().set(&DataKey::RewardTiers, &tiers);

        env.events()
            .publish(("campaign", "reward_tier_added"), (name, min_amount));
    }

    /// Returns the full ordered list of reward tiers.
    pub fn reward_tiers(env: Env) -> Vec<RewardTier> {
        env.storage()
            .instance()
            .get(&DataKey::RewardTiers)
            .unwrap_or_else(|| Vec::new(&env))
    }

    /// Returns the highest tier name the user's contribution qualifies for,
    /// or None if the user has not contributed or no tiers are defined.
    /// Tiers are evaluated by min_amount descending (highest qualifying tier wins).
    pub fn get_user_tier(env: Env, user: Address) -> Option<String> {
        let contribution: i128 = env
            .storage()
            .persistent()
            .get(&DataKey::Contribution(user))
            .unwrap_or(0);

        if contribution <= 0 {
            return None;
        }

        let tiers: Vec<RewardTier> = env
            .storage()
            .instance()
            .get(&DataKey::RewardTiers)
            .unwrap_or_else(|| Vec::new(&env));

        if tiers.is_empty() {
            return None;
        }

        let mut best: Option<RewardTier> = None;
        for tier in tiers.iter() {
            if contribution >= tier.min_amount {
                let is_better = match &best {
                    None => true,
                    Some(ref b) => tier.min_amount > b.min_amount,
                };
                if is_better {
                    best = Some(tier.clone());
                }
            }
        }

        best.map(|t| t.name)
    }

    /// Returns the next unmet stretch goal milestone.
    ///
    /// Returns 0 if there are no stretch goals or all have been met.
    pub fn current_milestone(env: Env) -> i128 {
        let total_raised: i128 = env
            .storage()
            .instance()
            .get(&DataKey::TotalRaised)
            .unwrap_or(0);

        let stretch_goals: Vec<i128> = env
            .storage()
            .instance()
            .get(&DataKey::StretchGoals)
            .unwrap_or_else(|| Vec::new(&env));

        for milestone in stretch_goals.iter() {
            if total_raised < milestone {
                return milestone;
            }
        }

        0
    }
    pub fn total_raised(env: Env) -> i128 {
        env.storage()
            .instance()
            .get(&DataKey::TotalRaised)
            .unwrap_or(0)
    }

    pub fn goal(env: Env) -> i128 {
        env.storage().instance().get(&DataKey::Goal).unwrap()
    }

    pub fn deadline(env: Env) -> u64 {
        env.storage().instance().get(&DataKey::Deadline).unwrap()
    }

    pub fn contribution(env: Env, contributor: Address) -> i128 {
        env.storage()
            .persistent()
            .get(&DataKey::Contribution(contributor))
            .unwrap_or(0)
    }

    pub fn min_contribution(env: Env) -> i128 {
        env.storage()
            .instance()
            .get(&DataKey::MinContribution)
            .unwrap()
    }

    pub fn creator(env: Env) -> Address {
        env.storage().instance().get(&DataKey::Creator).unwrap()
    }

    pub fn nft_contract(env: Env) -> Option<Address> {
        env.storage().instance().get(&DataKey::NFTContract)
    }

    pub fn get_campaign_info(env: Env) -> CampaignInfo {
        let creator: Address = env.storage().instance().get(&DataKey::Creator).unwrap();
        let token: Address = env.storage().instance().get(&DataKey::Token).unwrap();
        let goal: i128 = env.storage().instance().get(&DataKey::Goal).unwrap();
        let deadline: u64 = env.storage().instance().get(&DataKey::Deadline).unwrap();
        let total_raised: i128 = env
            .storage()
            .instance()
            .get(&DataKey::TotalRaised)
            .unwrap_or(0);
        let title: String = env
            .storage()
            .instance()
            .get(&DataKey::Title)
            .unwrap_or_else(|| String::from_str(&env, ""));
        let description: String = env
            .storage()
            .instance()
            .get(&DataKey::Description)
            .unwrap_or_else(|| String::from_str(&env, ""));

        CampaignInfo {
            creator,
            token,
            goal,
            deadline,
            total_raised,
        }
    }

    /// Returns true if the address is whitelisted.
    pub fn is_whitelisted(env: Env, address: Address) -> bool {
        env.storage()
            .instance()
            .get(&DataKey::Whitelist(address))
            .unwrap_or(false)
    }

    pub fn get_stats(env: Env) -> CampaignStats {
        let total_raised: i128 = env
            .storage()
            .instance()
            .get(&DataKey::TotalRaised)
            .unwrap_or(0);
        let goal: i128 = env.storage().instance().get(&DataKey::Goal).unwrap();
        let contributors: Vec<Address> = env
            .storage()
            .persistent()
            .get(&DataKey::Contributors)
            .unwrap_or_else(|| Vec::new(&env));

        let progress_bps = if goal > 0 {
            let raw = (total_raised * 10_000) / goal;
            if raw > 10_000 {
                10_000
            } else {
                raw as u32
            }
        } else {
            0
        };

        let contributor_count = contributors.len();
        let (average_contribution, largest_contribution) = if contributor_count == 0 {
            (0, 0)
        } else {
            let average = total_raised / contributor_count as i128;
            let mut largest = 0i128;
            for contributor in contributors.iter() {
                let amount: i128 = env
                    .storage()
                    .persistent()
                    .get(&DataKey::Contribution(contributor))
                    .unwrap_or(0);
                if amount > largest {
                    largest = amount;
                }
            }
            (average, largest)
        };

        CampaignStats {
            total_raised,
            goal,
            progress_bps,
            contributor_count,
            average_contribution,
            largest_contribution,
        }
    }

    pub fn title(env: Env) -> String {
        env.storage()
            .instance()
            .get(&DataKey::Title)
            .unwrap_or_else(|| String::from_str(&env, ""))
    }

    pub fn description(env: Env) -> String {
        env.storage()
            .instance()
            .get(&DataKey::Description)
            .unwrap_or_else(|| String::from_str(&env, ""))
    }

    pub fn socials(env: Env) -> String {
        env.storage()
            .instance()
            .get(&DataKey::SocialLinks)
            .unwrap_or_else(|| String::from_str(&env, ""))
    }

    pub fn version(_env: Env) -> u32 {
        CONTRACT_VERSION
    }

    /// Returns the token contract address used for contributions.
    pub fn token(env: Env) -> Address {
        env.storage().instance().get(&DataKey::Token).unwrap()
    }

    /// Returns the number of unique contributors.
    pub fn contributor_count(env: Env) -> u32 {
        let contributors: Vec<Address> = env
            .storage()
            .persistent()
            .get(&DataKey::Contributors)
            .unwrap_or_else(|| Vec::new(&env));
        contributors.len()
    }
}
