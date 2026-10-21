//! Multi-Token Vault
//!
//! A vault that accepts deposits of multiple distinct token types, identified
//! by an `Address` (the token contract address).  Each user has an independent
//! balance per token.  The vault tracks a global `vault_total` per token.
//!
//! ## Entry-points
//!
//! | Function       | Description                                                  |
//! |----------------|--------------------------------------------------------------|
//! | `deposit`      | Deposit `amount` of `token` on behalf of `depositor`.        |
//! | `withdraw`     | Withdraw `amount` of `token` from `depositor`'s balance.     |
//! | `vault_total`  | Return the total amount of `token` held across all users.    |
//! | `balance`      | Return `depositor`'s current balance for `token`.            |
//!
//! ## Invariants enforced by the contract
//!
//! * `withdraw` panics with `Error::InsufficientBalance` when
//!   `amount > depositor_balance`.
//! * After a successful `withdraw(depositor, token, amount)`, `vault_total(token)`
//!   decreases by exactly `amount`.

#![no_std]

use soroban_sdk::{contract, contracterror, contractimpl, contracttype, Address, Env};

// ── storage keys ──────────────────────────────────────────────────────────────

/// Storage keys used by the vault.
///
/// `Balance(depositor, token)` — per-user balance for a given token.
/// `VaultTotal(token)`         — aggregate vault balance for a given token.
#[contracttype]
#[derive(Clone)]
pub enum DataKey {
    /// Individual user balance: `(depositor_address, token_address)`.
    Balance(Address, Address),
    /// Sum of all user deposits for a single token.
    VaultTotal(Address),
}

// ── errors ────────────────────────────────────────────────────────────────────

#[contracterror]
#[derive(Copy, Clone, Debug, Eq, PartialEq, PartialOrd, Ord)]
#[repr(u32)]
pub enum Error {
    /// `amount` must be strictly positive.
    InvalidAmount = 1,
    /// Requested withdrawal exceeds the depositor's current balance.
    InsufficientBalance = 2,
}

// ── contract ──────────────────────────────────────────────────────────────────

#[contract]
pub struct MultiTokenVault;

#[contractimpl]
impl MultiTokenVault {
    // ── write operations ───────────────────────────────────────────────────

    /// Deposit `amount` of `token` into the vault on behalf of `depositor`.
    ///
    /// Requires authorization from `depositor`.
    ///
    /// # Errors
    ///
    /// Returns [`Error::InvalidAmount`] when `amount ≤ 0`.
    pub fn deposit(env: Env, depositor: Address, token: Address, amount: i128) -> Result<(), Error> {
        if amount <= 0 {
            return Err(Error::InvalidAmount);
        }
        depositor.require_auth();

        // Update per-user balance
        let balance_key = DataKey::Balance(depositor.clone(), token.clone());
        let current_balance: i128 = env
            .storage()
            .persistent()
            .get(&balance_key)
            .unwrap_or(0);
        env.storage()
            .persistent()
            .set(&balance_key, &(current_balance + amount));

        // Update vault total
        let total_key = DataKey::VaultTotal(token.clone());
        let current_total: i128 = env
            .storage()
            .persistent()
            .get(&total_key)
            .unwrap_or(0);
        env.storage()
            .persistent()
            .set(&total_key, &(current_total + amount));

        Ok(())
    }

    /// Withdraw `amount` of `token` from the vault for `depositor`.
    ///
    /// Requires authorization from `depositor`.
    ///
    /// # Errors
    ///
    /// * [`Error::InvalidAmount`] — `amount ≤ 0`.
    /// * [`Error::InsufficientBalance`] — `amount > depositor_balance`.
    pub fn withdraw(
        env: Env,
        depositor: Address,
        token: Address,
        amount: i128,
    ) -> Result<(), Error> {
        if amount <= 0 {
            return Err(Error::InvalidAmount);
        }
        depositor.require_auth();

        let balance_key = DataKey::Balance(depositor.clone(), token.clone());
        let current_balance: i128 = env
            .storage()
            .persistent()
            .get(&balance_key)
            .unwrap_or(0);

        if amount > current_balance {
            return Err(Error::InsufficientBalance);
        }

        // Update per-user balance
        env.storage()
            .persistent()
            .set(&balance_key, &(current_balance - amount));

        // Update vault total
        let total_key = DataKey::VaultTotal(token.clone());
        let current_total: i128 = env
            .storage()
            .persistent()
            .get(&total_key)
            .unwrap_or(0);
        env.storage()
            .persistent()
            .set(&total_key, &(current_total - amount));

        Ok(())
    }

    // ── read operations ────────────────────────────────────────────────────

    /// Return the total amount of `token` currently held in the vault across
    /// all depositors.
    pub fn vault_total(env: Env, token: Address) -> i128 {
        env.storage()
            .persistent()
            .get(&DataKey::VaultTotal(token))
            .unwrap_or(0)
    }

    /// Return the `depositor`'s current balance for `token`.
    pub fn balance(env: Env, depositor: Address, token: Address) -> i128 {
        env.storage()
            .persistent()
            .get(&DataKey::Balance(depositor, token))
            .unwrap_or(0)
    }
}

// ── tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use soroban_sdk::{testutils::Address as _, Env};

    /// Stand-in for a token address — just a random `Address`.
    fn make_token(env: &Env) -> Address {
        Address::generate(env)
    }

    /// Register the vault contract and return a ready-to-use client.
    fn setup() -> (Env, MultiTokenVaultClient<'static>) {
        let env = Env::default();
        env.mock_all_auths();
        let contract_id = env.register(MultiTokenVault, ());
        let client = MultiTokenVaultClient::new(&env, &contract_id);
        (env, client)
    }

    // ── deposit ────────────────────────────────────────────────────────────

    #[test]
    fn test_deposit_increases_user_balance() {
        let (env, client) = setup();
        let alice = Address::generate(&env);
        let token = make_token(&env);

        client.deposit(&alice, &token, &1000);
        assert_eq!(client.balance(&alice, &token), 1000);
    }

    #[test]
    fn test_deposit_increases_vault_total() {
        let (env, client) = setup();
        let alice = Address::generate(&env);
        let token = make_token(&env);

        client.deposit(&alice, &token, &500);
        assert_eq!(client.vault_total(&token), 500);
    }

    #[test]
    fn test_multiple_depositors_accumulate_vault_total() {
        let (env, client) = setup();
        let alice = Address::generate(&env);
        let bob = Address::generate(&env);
        let token = make_token(&env);

        client.deposit(&alice, &token, &600);
        client.deposit(&bob, &token, &400);

        assert_eq!(client.vault_total(&token), 1000);
        assert_eq!(client.balance(&alice, &token), 600);
        assert_eq!(client.balance(&bob, &token), 400);
    }

    #[test]
    fn test_deposit_zero_is_rejected() {
        let (env, client) = setup();
        let alice = Address::generate(&env);
        let token = make_token(&env);

        let result = client.try_deposit(&alice, &token, &0);
        assert_eq!(result, Err(Ok(Error::InvalidAmount)));
    }

    #[test]
    fn test_deposit_negative_is_rejected() {
        let (env, client) = setup();
        let alice = Address::generate(&env);
        let token = make_token(&env);

        let result = client.try_deposit(&alice, &token, &-1);
        assert_eq!(result, Err(Ok(Error::InvalidAmount)));
    }

    #[test]
    fn test_different_tokens_are_tracked_independently() {
        let (env, client) = setup();
        let alice = Address::generate(&env);
        let token_a = make_token(&env);
        let token_b = make_token(&env);

        client.deposit(&alice, &token_a, &300);
        client.deposit(&alice, &token_b, &700);

        assert_eq!(client.balance(&alice, &token_a), 300);
        assert_eq!(client.balance(&alice, &token_b), 700);
        assert_eq!(client.vault_total(&token_a), 300);
        assert_eq!(client.vault_total(&token_b), 700);
    }

    // ── withdraw ───────────────────────────────────────────────────────────

    /// **Core rule**: A withdraw above the user's balance must fail with
    /// `Error::InsufficientBalance` (contract error, not a panic).
    #[test]
    fn test_withdraw_above_balance_fails_with_contract_error() {
        let (env, client) = setup();
        let alice = Address::generate(&env);
        let token = make_token(&env);

        // Alice has 200 tokens in the vault.
        client.deposit(&alice, &token, &200);

        // Attempt to withdraw 201 — must return a contract error, not panic.
        let result = client.try_withdraw(&alice, &token, &201);
        assert_eq!(result, Err(Ok(Error::InsufficientBalance)));

        // State must be unchanged after the failed withdrawal.
        assert_eq!(client.balance(&alice, &token), 200);
        assert_eq!(client.vault_total(&token), 200);
    }

    /// **Core rule**: A valid withdraw decreases `vault_total` by exactly the
    /// withdrawn amount.
    #[test]
    fn test_valid_withdraw_decreases_vault_total() {
        let (env, client) = setup();
        let alice = Address::generate(&env);
        let token = make_token(&env);

        client.deposit(&alice, &token, &1000);
        assert_eq!(client.vault_total(&token), 1000);

        client.withdraw(&alice, &token, &350);

        // vault_total must decrease by exactly 350.
        assert_eq!(client.vault_total(&token), 650);
        // User balance must also decrease by exactly 350.
        assert_eq!(client.balance(&alice, &token), 650);
    }

    #[test]
    fn test_full_withdraw_empties_vault_total() {
        let (env, client) = setup();
        let alice = Address::generate(&env);
        let token = make_token(&env);

        client.deposit(&alice, &token, &500);
        client.withdraw(&alice, &token, &500);

        assert_eq!(client.vault_total(&token), 0);
        assert_eq!(client.balance(&alice, &token), 0);
    }

    #[test]
    fn test_withdraw_from_empty_balance_fails_with_contract_error() {
        let (env, client) = setup();
        let alice = Address::generate(&env);
        let token = make_token(&env);

        // Alice has never deposited — balance is 0.
        let result = client.try_withdraw(&alice, &token, &1);
        assert_eq!(result, Err(Ok(Error::InsufficientBalance)));
    }

    #[test]
    fn test_withdraw_zero_is_rejected() {
        let (env, client) = setup();
        let alice = Address::generate(&env);
        let token = make_token(&env);

        client.deposit(&alice, &token, &100);
        let result = client.try_withdraw(&alice, &token, &0);
        assert_eq!(result, Err(Ok(Error::InvalidAmount)));
    }

    #[test]
    fn test_withdraw_negative_is_rejected() {
        let (env, client) = setup();
        let alice = Address::generate(&env);
        let token = make_token(&env);

        client.deposit(&alice, &token, &100);
        let result = client.try_withdraw(&alice, &token, &-50);
        assert_eq!(result, Err(Ok(Error::InvalidAmount)));
    }

    /// Vault totals for different tokens must not interfere: withdrawing token A
    /// must not affect `vault_total` for token B.
    #[test]
    fn test_withdraw_does_not_affect_other_tokens() {
        let (env, client) = setup();
        let alice = Address::generate(&env);
        let token_a = make_token(&env);
        let token_b = make_token(&env);

        client.deposit(&alice, &token_a, &400);
        client.deposit(&alice, &token_b, &600);

        client.withdraw(&alice, &token_a, &100);

        // token_a total drops by 100.
        assert_eq!(client.vault_total(&token_a), 300);
        // token_b total is completely unaffected.
        assert_eq!(client.vault_total(&token_b), 600);
    }

    /// When two users hold the same token, withdrawing one user's share must
    /// decrease `vault_total` without touching the other user's balance.
    #[test]
    fn test_withdraw_only_affects_correct_user_balance() {
        let (env, client) = setup();
        let alice = Address::generate(&env);
        let bob = Address::generate(&env);
        let token = make_token(&env);

        client.deposit(&alice, &token, &700);
        client.deposit(&bob, &token, &300);
        assert_eq!(client.vault_total(&token), 1000);

        client.withdraw(&alice, &token, &200);

        // vault_total decreases by 200.
        assert_eq!(client.vault_total(&token), 800);
        // Alice's balance decreases by 200.
        assert_eq!(client.balance(&alice, &token), 500);
        // Bob's balance is unchanged.
        assert_eq!(client.balance(&bob, &token), 300);
    }

    // ── vault_total view ───────────────────────────────────────────────────

    #[test]
    fn test_vault_total_starts_at_zero() {
        let (env, client) = setup();
        let token = make_token(&env);
        assert_eq!(client.vault_total(&token), 0);
    }
}
