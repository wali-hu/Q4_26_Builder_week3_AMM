use {
    amm::{state::Config, CONFIG_SEED, LP_SEED},
    anchor_lang::{
        prelude::Pubkey,
        solana_program::{instruction::Instruction, system_program},
        AccountDeserialize, InstructionData, ToAccountMetas,
    },
    anchor_spl::associated_token::{self, get_associated_token_address},
    litesvm::LiteSVM,
    litesvm_token::{
        get_spl_account,
        spl_token::state::{Account as TokenAccount, Mint},
        CreateAssociatedTokenAccount, CreateMint, MintTo,
    },
    solana_keypair::Keypair,
    solana_message::{Message, VersionedMessage},
    solana_signer::Signer,
    solana_transaction::versioned::VersionedTransaction,
};

const SEED: u64 = 7;
const START: u64 = 1_000_000_000_000; // every user starts with this much X and Y

// ---------- test world ----------

struct World {
    svm: LiteSVM,
    pid: Pubkey,
    admin: Keypair,
    treasury: Pubkey,
    alice: Keypair,
    bob: Keypair,
    mint_x: Pubkey,
    mint_y: Pubkey,
    config: Pubkey,
    mint_lp: Pubkey,
}

impl World {
    fn vault(&self, mint: &Pubkey) -> Pubkey {
        get_associated_token_address(&self.config, mint)
    }
    fn treasury_ata(&self, mint: &Pubkey) -> Pubkey {
        get_associated_token_address(&self.treasury, mint)
    }
    fn ata(&self, owner: &Pubkey, mint: &Pubkey) -> Pubkey {
        get_associated_token_address(owner, mint)
    }
    fn reserves(&self) -> (u64, u64) {
        (bal(&self.svm, &self.vault(&self.mint_x)), bal(&self.svm, &self.vault(&self.mint_y)))
    }
    fn lp_supply(&self) -> u64 {
        get_spl_account::<Mint>(&self.svm, &self.mint_lp).unwrap().supply
    }
    fn config_state(&self) -> Config {
        let acc = self.svm.get_account(&self.config).unwrap();
        Config::try_deserialize(&mut acc.data.as_slice()).unwrap()
    }
}

/// Two mints (6 decimals), Alice and Bob hold START of each. No pool yet.
fn setup() -> World {
    let pid = amm::id();
    let mut svm = LiteSVM::new();
    let bytes = include_bytes!(concat!(env!("CARGO_TARGET_TMPDIR"), "/../deploy/amm.so"));
    svm.add_program(pid, bytes).unwrap();

    let (admin, alice, bob) = (Keypair::new(), Keypair::new(), Keypair::new());
    let treasury = Keypair::new().pubkey();
    for k in [&admin, &alice, &bob] {
        svm.airdrop(&k.pubkey(), 100_000_000_000).unwrap();
    }
    let mint_x = CreateMint::new(&mut svm, &admin).decimals(6).send().unwrap();
    let mint_y = CreateMint::new(&mut svm, &admin).decimals(6).send().unwrap();
    for user in [&alice, &bob] {
        for mint in [&mint_x, &mint_y] {
            let ata = CreateAssociatedTokenAccount::new(&mut svm, user, mint).send().unwrap();
            MintTo::new(&mut svm, &admin, mint, &ata, START).send().unwrap();
        }
    }
    let config = Pubkey::find_program_address(&[CONFIG_SEED, &SEED.to_le_bytes()], &pid).0;
    let mint_lp = Pubkey::find_program_address(&[LP_SEED, config.as_ref()], &pid).0;
    World { svm, pid, admin, treasury, alice, bob, mint_x, mint_y, config, mint_lp }
}

/// Pool with the given fees; nothing deposited yet.
fn world(fee_bps: u16, protocol_fee_bps: u16) -> World {
    let mut w = setup();
    let ix = ix_initialize(&w, w.mint_x, w.mint_y, fee_bps, protocol_fee_bps);
    let admin = w.admin.insecure_clone();
    send(&mut w.svm, &admin, ix).unwrap();
    w
}

fn send(svm: &mut LiteSVM, signer: &Keypair, ix: Instruction) -> Result<(), String> {
    let msg = Message::new_with_blockhash(&[ix], Some(&signer.pubkey()), &svm.latest_blockhash());
    let tx = VersionedTransaction::try_new(VersionedMessage::Legacy(msg), &[signer]).unwrap();
    let res = svm
        .send_transaction(tx)
        .map(|_| ())
        .map_err(|e| format!("{:?}\n{}", e.err, e.meta.logs.join("\n")));
    svm.expire_blockhash();
    res
}

fn assert_error(res: Result<(), String>, code: u32) {
    let err = res.expect_err("transaction should have failed");
    assert!(err.contains(&format!("Custom({code})")), "expected {code}, got:\n{err}");
}

fn bal(svm: &LiteSVM, ata: &Pubkey) -> u64 {
    get_spl_account::<TokenAccount>(svm, ata).map(|a| a.amount).unwrap_or(0)
}

// ---------- instruction builders ----------

fn ix_initialize(w: &World, mint_x: Pubkey, mint_y: Pubkey, fee_bps: u16, protocol_fee_bps: u16) -> Instruction {
    Instruction::new_with_bytes(
        w.pid,
        &amm::instruction::Initialize { seed: SEED, fee_bps, protocol_fee_bps }.data(),
        amm::accounts::Initialize {
            authority: w.admin.pubkey(),
            treasury: w.treasury,
            mint_x,
            mint_y,
            config: w.config,
            mint_lp: w.mint_lp,
            vault_x: get_associated_token_address(&w.config, &mint_x),
            vault_y: get_associated_token_address(&w.config, &mint_y),
            treasury_x: get_associated_token_address(&w.treasury, &mint_x),
            treasury_y: get_associated_token_address(&w.treasury, &mint_y),
            associated_token_program: associated_token::ID,
            token_program: anchor_spl::token::ID,
            system_program: system_program::ID,
        }
        .to_account_metas(None),
    )
}

fn ix_deposit(w: &World, user: Pubkey, lp_amount: u64, max_x: u64, max_y: u64) -> Instruction {
    Instruction::new_with_bytes(
        w.pid,
        &amm::instruction::Deposit { lp_amount, max_x, max_y }.data(),
        amm::accounts::Deposit {
            user,
            config: w.config,
            mint_x: w.mint_x,
            mint_y: w.mint_y,
            mint_lp: w.mint_lp,
            vault_x: w.vault(&w.mint_x),
            vault_y: w.vault(&w.mint_y),
            user_x: w.ata(&user, &w.mint_x),
            user_y: w.ata(&user, &w.mint_y),
            user_lp: w.ata(&user, &w.mint_lp),
            associated_token_program: associated_token::ID,
            token_program: anchor_spl::token::ID,
            system_program: system_program::ID,
        }
        .to_account_metas(None),
    )
}

fn ix_swap_with(w: &World, user: Pubkey, mint_x: Pubkey, x_to_y: bool, amount_in: u64, min_out: u64) -> Instruction {
    Instruction::new_with_bytes(
        w.pid,
        &amm::instruction::Swap { x_to_y, amount_in, min_out }.data(),
        amm::accounts::Swap {
            user,
            config: w.config,
            treasury: w.treasury,
            // only the mint is swapped: every token account is the real one
            mint_x,
            mint_y: w.mint_y,
            vault_x: w.vault(&w.mint_x),
            vault_y: w.vault(&w.mint_y),
            user_x: w.ata(&user, &w.mint_x),
            user_y: w.ata(&user, &w.mint_y),
            treasury_x: w.treasury_ata(&w.mint_x),
            treasury_y: w.treasury_ata(&w.mint_y),
            associated_token_program: associated_token::ID,
            token_program: anchor_spl::token::ID,
            system_program: system_program::ID,
        }
        .to_account_metas(None),
    )
}

fn ix_swap(w: &World, user: Pubkey, x_to_y: bool, amount_in: u64, min_out: u64) -> Instruction {
    ix_swap_with(w, user, w.mint_x, x_to_y, amount_in, min_out)
}

fn ix_withdraw(w: &World, user: Pubkey, lp_amount: u64, min_x: u64, min_y: u64) -> Instruction {
    Instruction::new_with_bytes(
        w.pid,
        &amm::instruction::Withdraw { lp_amount, min_x, min_y }.data(),
        amm::accounts::Withdraw {
            user,
            config: w.config,
            mint_x: w.mint_x,
            mint_y: w.mint_y,
            mint_lp: w.mint_lp,
            vault_x: w.vault(&w.mint_x),
            vault_y: w.vault(&w.mint_y),
            user_x: w.ata(&user, &w.mint_x),
            user_y: w.ata(&user, &w.mint_y),
            user_lp: w.ata(&user, &w.mint_lp),
            token_program: anchor_spl::token::ID,
        }
        .to_account_metas(None),
    )
}

fn ix_update_fees(w: &World, authority: Pubkey, fee_bps: u16, protocol_fee_bps: u16) -> Instruction {
    Instruction::new_with_bytes(
        w.pid,
        &amm::instruction::UpdateFees { fee_bps, protocol_fee_bps }.data(),
        amm::accounts::Admin { authority, config: w.config }.to_account_metas(None),
    )
}

fn ix_set_locked(w: &World, authority: Pubkey, locked: bool) -> Instruction {
    Instruction::new_with_bytes(
        w.pid,
        &amm::instruction::SetLocked { locked }.data(),
        amm::accounts::Admin { authority, config: w.config }.to_account_metas(None),
    )
}

// small helpers that send and unwrap
fn deposit(w: &mut World, who: &Keypair, lp: u64, max_x: u64, max_y: u64) {
    let ix = ix_deposit(w, who.pubkey(), lp, max_x, max_y);
    send(&mut w.svm, who, ix).unwrap();
}
fn swap(w: &mut World, who: &Keypair, x_to_y: bool, amount_in: u64, min_out: u64) {
    let ix = ix_swap(w, who.pubkey(), x_to_y, amount_in, min_out);
    send(&mut w.svm, who, ix).unwrap();
}

/// Alice seeds the pool with 1,000 X and 4,000 Y (6 decimals): LP = sqrt(x*y) = 2,000.
fn seeded(fee_bps: u16, protocol_fee_bps: u16) -> (World, Keypair, Keypair) {
    let mut w = world(fee_bps, protocol_fee_bps);
    let (a, b) = (w.alice.insecure_clone(), w.bob.insecure_clone());
    deposit(&mut w, &a, 0, 1_000_000_000, 4_000_000_000);
    (w, a, b)
}

// ---------- initialize ----------

#[test]
fn initialize_creates_config_vaults_lp_mint_and_treasury() {
    let w = world(30, 2_000);
    let c = w.config_state();
    assert_eq!(c.authority, w.admin.pubkey());
    assert_eq!(c.treasury, w.treasury);
    assert_eq!((c.mint_x, c.mint_y), (w.mint_x, w.mint_y));
    assert_eq!((c.fee_bps, c.protocol_fee_bps, c.locked), (30, 2_000, false));

    let lp = get_spl_account::<Mint>(&w.svm, &w.mint_lp).unwrap();
    assert_eq!(lp.mint_authority, Some(w.config).into(), "only the config PDA mints LP");
    assert_eq!(lp.supply, 0);
    for acc in [w.vault(&w.mint_x), w.vault(&w.mint_y), w.treasury_ata(&w.mint_x), w.treasury_ata(&w.mint_y)] {
        assert!(w.svm.get_account(&acc).is_some());
    }
    // vault owner is the config PDA, treasury ATA owner is the treasury wallet
    let vx = get_spl_account::<TokenAccount>(&w.svm, &w.vault(&w.mint_x)).unwrap();
    assert_eq!(vx.owner, w.config);
    let tx = get_spl_account::<TokenAccount>(&w.svm, &w.treasury_ata(&w.mint_x)).unwrap();
    assert_eq!(tx.owner, w.treasury);
}

#[test]
fn initialize_with_fee_above_max_fails_6003() {
    let mut w = setup();
    let admin = w.admin.insecure_clone();
    let ix = ix_initialize(&w, w.mint_x, w.mint_y, 1_001, 0);
    assert_error(send(&mut w.svm, &admin, ix), 6003);
    let ix = ix_initialize(&w, w.mint_x, w.mint_y, 30, 10_001);
    assert_error(send(&mut w.svm, &admin, ix), 6003);
}

#[test]
fn initialize_with_identical_mints_fails() {
    let mut w = setup();
    let admin = w.admin.insecure_clone();
    // vault_x and vault_y become the same account: Anchor's init of the second fails
    // before the handler can return 6004 IdenticalMints, so just assert it fails.
    let ix = ix_initialize(&w, w.mint_x, w.mint_x, 30, 0);
    assert!(send(&mut w.svm, &admin, ix).is_err());
}

// ---------- deposit ----------

#[test]
fn first_deposit_mints_sqrt_xy_lp() {
    let (w, alice, _) = seeded(30, 2_000);
    // sqrt(1_000e6 * 4_000e6) = 2_000e6
    assert_eq!(bal(&w.svm, &w.ata(&alice.pubkey(), &w.mint_lp)), 2_000_000_000);
    assert_eq!(w.reserves(), (1_000_000_000, 4_000_000_000));
}

#[test]
fn second_deposit_keeps_the_ratio_and_rounds_up() {
    let (mut w, _, bob) = seeded(30, 2_000);
    // 10% more LP needs 10% more of each reserve
    deposit(&mut w, &bob, 200_000_000, 100_000_000, 400_000_000);
    assert_eq!(w.reserves(), (1_100_000_000, 4_400_000_000));
    assert_eq!(bal(&w.svm, &w.ata(&bob.pubkey(), &w.mint_lp)), 200_000_000);
    assert_eq!(w.lp_supply(), 2_200_000_000);
}

#[test]
fn deposit_above_max_fails_6001_slippage() {
    let (mut w, _, bob) = seeded(30, 2_000);
    // 200 LP needs exactly 100 X; allow only 99.999999 X
    let ix = ix_deposit(&w, bob.pubkey(), 200_000_000, 99_999_999, 400_000_000);
    assert_error(send(&mut w.svm, &bob, ix), 6001);
    let ix = ix_deposit(&w, bob.pubkey(), 200_000_000, 100_000_000, 399_999_999);
    assert_error(send(&mut w.svm, &bob, ix), 6001);
}

#[test]
fn first_deposit_below_min_lp_fails_6001_slippage() {
    let mut w = world(30, 2_000);
    let alice = w.alice.insecure_clone();
    // sqrt(1e9 * 4e9) = 2e9 LP, but Alice asks for at least 2e9 + 1
    let ix = ix_deposit(&w, alice.pubkey(), 2_000_000_001, 1_000_000_000, 4_000_000_000);
    assert_error(send(&mut w.svm, &alice, ix), 6001);
}

#[test]
fn deposit_zero_fails_6000() {
    let mut w = world(30, 2_000);
    let alice = w.alice.insecure_clone();
    let ix = ix_deposit(&w, alice.pubkey(), 0, 0, 4_000_000_000);
    assert_error(send(&mut w.svm, &alice, ix), 6000);
    let (mut w, _, bob) = seeded(30, 2_000);
    let ix = ix_deposit(&w, bob.pubkey(), 0, 100, 100);
    assert_error(send(&mut w.svm, &bob, ix), 6000);
}

// ---------- swap ----------

#[test]
fn slide_example_k_600_without_fees() {
    // Slides: x = 20, y = 30, k = 600. Swap 5 X -> 6 Y, then 5 X -> 4 Y.
    let mut w = world(0, 0);
    let (alice, bob) = (w.alice.insecure_clone(), w.bob.insecure_clone());
    deposit(&mut w, &alice, 0, 20, 30);
    let y0 = bal(&w.svm, &w.ata(&bob.pubkey(), &w.mint_y));
    swap(&mut w, &bob, true, 5, 6);
    assert_eq!(bal(&w.svm, &w.ata(&bob.pubkey(), &w.mint_y)) - y0, 6);
    assert_eq!(w.reserves(), (25, 24), "k is still 600");
    swap(&mut w, &bob, true, 5, 4);
    assert_eq!(bal(&w.svm, &w.ata(&bob.pubkey(), &w.mint_y)) - y0, 10);
    assert_eq!(w.reserves(), (30, 20), "k is still 600");
}

#[test]
fn swap_x_to_y_splits_fee_between_pool_and_treasury() {
    // fee 30 bps, protocol share 20% of the fee
    let (mut w, _, bob) = seeded(30, 2_000);
    let (x0, y0) = w.reserves();
    let amount_in = 10_000_000; // 10 X
    let bob_y0 = bal(&w.svm, &w.ata(&bob.pubkey(), &w.mint_y));
    swap(&mut w, &bob, true, amount_in, 0);

    // fee = ceil(10_000_000 * 30 / 10_000) = 30_000; protocol = 20% = 6_000; LPs keep 24_000
    let in_after_fee = amount_in - 30_000;
    let expected_out = (y0 as u128 * in_after_fee as u128 / (x0 as u128 + in_after_fee as u128)) as u64;
    let bob_got = bal(&w.svm, &w.ata(&bob.pubkey(), &w.mint_y)) - bob_y0;
    assert_eq!(bob_got, expected_out);
    assert_eq!(bob_got, 39_486_321); // 4000 * 9.97 / 1009.97 Y, rounded down
    assert_eq!(bal(&w.svm, &w.treasury_ata(&w.mint_x)), 6_000, "treasury got the protocol fee");
    assert_eq!(w.reserves(), (x0 + amount_in - 6_000, y0 - expected_out));
    // k grew: the LP part of the fee stayed in the pool
    let (x1, y1) = w.reserves();
    assert!(x1 as u128 * y1 as u128 > x0 as u128 * y0 as u128);
}

#[test]
fn swap_y_to_x_charges_the_fee_too() {
    let (mut w, _, bob) = seeded(30, 2_000);
    let (x0, y0) = w.reserves();
    let bob_x0 = bal(&w.svm, &w.ata(&bob.pubkey(), &w.mint_x));
    swap(&mut w, &bob, false, 40_000_000, 0); // 40 Y in
    // fee = 120_000, protocol = 24_000
    let in_after_fee = 40_000_000 - 120_000u64;
    let expected = (x0 as u128 * in_after_fee as u128 / (y0 as u128 + in_after_fee as u128)) as u64;
    assert_eq!(bal(&w.svm, &w.ata(&bob.pubkey(), &w.mint_x)) - bob_x0, expected);
    assert_eq!(bal(&w.svm, &w.treasury_ata(&w.mint_y)), 24_000);
}

#[test]
fn swap_below_min_out_fails_6001_slippage() {
    let (mut w, _, bob) = seeded(30, 2_000);
    // the real output is 39_486_321; ask for one unit more
    let ix = ix_swap(&w, bob.pubkey(), true, 10_000_000, 39_486_322);
    assert_error(send(&mut w.svm, &bob, ix), 6001);
}

#[test]
fn swap_zero_fails_6000() {
    let (mut w, _, bob) = seeded(30, 2_000);
    let ix = ix_swap(&w, bob.pubkey(), true, 0, 0);
    assert_error(send(&mut w.svm, &bob, ix), 6000);
}

#[test]
fn swap_with_wrong_mint_fails_2014_token_mint() {
    let (mut w, _, bob) = seeded(30, 2_000);
    let admin = w.admin.insecure_clone();
    let fake = CreateMint::new(&mut w.svm, &admin).decimals(6).send().unwrap();
    // Fake mint_x with the real token accounts. user_x is init_if_needed, and Anchor
    // checks an init_if_needed account's mint while loading it, before has_one runs:
    // 2014 ConstraintTokenMint.
    let ix = ix_swap_with(&w, bob.pubkey(), fake, true, 1_000, 0);
    assert_error(send(&mut w.svm, &bob, ix), 2014);
}

#[test]
fn deposit_with_wrong_mint_fails_2001_has_one() {
    let (mut w, _, bob) = seeded(30, 2_000);
    let admin = w.admin.insecure_clone();
    let fake = CreateMint::new(&mut w.svm, &admin).decimals(6).send().unwrap();
    // Here no account before `config` validates the mint, so config's
    // has_one = mint_x fires first: 2001 ConstraintHasOne.
    let mut ix = ix_deposit(&w, bob.pubkey(), 200_000_000, 100_000_000, 400_000_000);
    ix.accounts[2].pubkey = fake; // [user, config, mint_x, ...]
    assert_error(send(&mut w.svm, &bob, ix), 2001);
}

#[test]
fn swap_on_empty_pool_fails_6005_no_liquidity() {
    let mut w = world(30, 2_000);
    let bob = w.bob.insecure_clone();
    let ix = ix_swap(&w, bob.pubkey(), true, 1_000, 0);
    assert_error(send(&mut w.svm, &bob, ix), 6005);
}

// ---------- withdraw ----------

#[test]
fn withdraw_returns_pro_rata_share_including_earned_fees() {
    let (mut w, alice, bob) = seeded(30, 2_000);
    swap(&mut w, &bob, true, 10_000_000, 0);
    let (x, y) = w.reserves();
    let lp = bal(&w.svm, &w.ata(&alice.pubkey(), &w.mint_lp));
    let ax0 = bal(&w.svm, &w.ata(&alice.pubkey(), &w.mint_x));
    let ay0 = bal(&w.svm, &w.ata(&alice.pubkey(), &w.mint_y));

    // Alice burns half her LP: gets half of each reserve (rounded down)
    let ix = ix_withdraw(&w, alice.pubkey(), lp / 2, x / 2, y / 2);
    send(&mut w.svm, &alice, ix).unwrap();
    assert_eq!(bal(&w.svm, &w.ata(&alice.pubkey(), &w.mint_x)) - ax0, x / 2);
    assert_eq!(bal(&w.svm, &w.ata(&alice.pubkey(), &w.mint_y)) - ay0, y / 2);
    assert_eq!(w.lp_supply(), lp / 2);
}

#[test]
fn withdraw_everything_empties_the_pool() {
    let (mut w, alice, _) = seeded(30, 2_000);
    let lp = bal(&w.svm, &w.ata(&alice.pubkey(), &w.mint_lp));
    let ix = ix_withdraw(&w, alice.pubkey(), lp, 1_000_000_000, 4_000_000_000);
    send(&mut w.svm, &alice, ix).unwrap();
    assert_eq!(w.reserves(), (0, 0));
    assert_eq!(w.lp_supply(), 0);
}

#[test]
fn withdraw_below_min_fails_6001_slippage() {
    let (mut w, alice, _) = seeded(30, 2_000);
    // half the LP gives exactly 500 X and 2_000 Y; ask for one more X
    let ix = ix_withdraw(&w, alice.pubkey(), 1_000_000_000, 500_000_001, 0);
    assert_error(send(&mut w.svm, &alice, ix), 6001);
    let ix = ix_withdraw(&w, alice.pubkey(), 1_000_000_000, 0, 2_000_000_001);
    assert_error(send(&mut w.svm, &alice, ix), 6001);
}

#[test]
fn withdraw_more_lp_than_owned_fails_token_insufficient_funds() {
    let (mut w, alice, bob) = seeded(30, 2_000);
    deposit(&mut w, &bob, 200_000_000, 100_000_000, 400_000_000);
    // Bob owns 200 LP and tries to burn 300 (TokenError::InsufficientFunds = 1)
    let ix = ix_withdraw(&w, bob.pubkey(), 300_000_000, 0, 0);
    assert_error(send(&mut w.svm, &bob, ix), 1);
    let _ = alice;
}

#[test]
fn withdraw_zero_fails_6000() {
    let (mut w, alice, _) = seeded(30, 2_000);
    let ix = ix_withdraw(&w, alice.pubkey(), 0, 0, 0);
    assert_error(send(&mut w.svm, &alice, ix), 6000);
}

// ---------- admin: fees and lock ----------

#[test]
fn authority_updates_fees_and_the_new_fee_applies() {
    let (mut w, _, bob) = seeded(30, 2_000);
    let admin = w.admin.insecure_clone();
    let ix = ix_update_fees(&w, admin.pubkey(), 100, 5_000); // 1%, half to treasury
    send(&mut w.svm, &admin, ix).unwrap();
    assert_eq!((w.config_state().fee_bps, w.config_state().protocol_fee_bps), (100, 5_000));
    swap(&mut w, &bob, true, 10_000_000, 0);
    // fee = 100_000, protocol = 50_000
    assert_eq!(bal(&w.svm, &w.treasury_ata(&w.mint_x)), 50_000);
}

#[test]
fn non_authority_cannot_update_fees_or_lock_2001() {
    let (mut w, _, bob) = seeded(30, 2_000);
    let ix = ix_update_fees(&w, bob.pubkey(), 0, 0);
    assert_error(send(&mut w.svm, &bob, ix), 2001);
    let ix = ix_set_locked(&w, bob.pubkey(), true);
    assert_error(send(&mut w.svm, &bob, ix), 2001);
}

#[test]
fn update_fees_out_of_range_fails_6003() {
    let (mut w, _, _) = seeded(30, 2_000);
    let admin = w.admin.insecure_clone();
    let ix = ix_update_fees(&w, admin.pubkey(), 1_001, 0);
    assert_error(send(&mut w.svm, &admin, ix), 6003);
}

#[test]
fn locked_pool_blocks_deposit_and_swap_6002_but_not_withdraw() {
    let (mut w, alice, bob) = seeded(30, 2_000);
    let admin = w.admin.insecure_clone();
    let ix = ix_set_locked(&w, admin.pubkey(), true);
    send(&mut w.svm, &admin, ix).unwrap();

    let ix = ix_swap(&w, bob.pubkey(), true, 1_000_000, 0);
    assert_error(send(&mut w.svm, &bob, ix), 6002);
    let ix = ix_deposit(&w, bob.pubkey(), 200_000_000, 100_000_000, 400_000_000);
    assert_error(send(&mut w.svm, &bob, ix), 6002);

    // LPs can still exit
    let ix = ix_withdraw(&w, alice.pubkey(), 1_000_000_000, 0, 0);
    send(&mut w.svm, &alice, ix).unwrap();

    // unlock and trading resumes
    let ix = ix_set_locked(&w, admin.pubkey(), false);
    send(&mut w.svm, &admin, ix).unwrap();
    swap(&mut w, &bob, true, 1_000_000, 0);
}
