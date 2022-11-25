//! A script for building the main state struct of the Bitcoin canister.
//!
//! Example run:
//!
//! cargo run --release -- \
//!   --canister-state canister.bin \
//!   --canister-state-dir ./canister_state \
//!   --network mainnet --stability-threshold 30 --stable-height 9999 \
//!   --unstable-blocks ./unstable_blocks
use bitcoin::{consensus::Decodable, Block as BitcoinBlock};
use clap::Parser;
use ic_btc_canister::{
    pre_upgrade,
    types::{Block, Config, Network, OutPoint, TxOut},
    unstable_blocks::{self, UnstableBlocks},
    with_state, with_state_mut,
};
use ic_btc_types::Height;
use ic_stable_structures::FileMemory;
use std::{
    collections::BTreeMap,
    fs::File,
    io::{BufRead, BufReader, Read},
    path::PathBuf,
};

#[derive(Parser, Debug)]
struct Args {
    /// The canister's pre-computed state.
    #[clap(long, value_hint = clap::ValueHint::DirPath)]
    canister_state: PathBuf,

    /// The directory containing the pre-computed canister memories.
    #[clap(long, value_hint = clap::ValueHint::DirPath)]
    canister_state_dir: PathBuf,

    /// The file containing the unstable blocks.
    #[clap(long, value_hint = clap::ValueHint::DirPath)]
    unstable_blocks: PathBuf,

    /// The bitcoin network.
    #[clap(long)]
    network: Network,

    /// The stability threshold to use.
    #[clap(long)]
    stability_threshold: u128,

    /// The stable height of the canister.
    #[clap(long)]
    stable_height: u32,
}

fn read_block(reader: &mut BufReader<File>) -> Block {
    let mut block = String::new();
    reader.read_line(&mut block).unwrap();
    let block = hex::decode(block.replace('\n', "")).unwrap();
    Block::new(BitcoinBlock::consensus_decode(block.as_slice()).unwrap())
}

fn main() {
    let args = Args::parse();

    // Set the memory of the canister.
    ic_btc_canister::memory::set_memory(FileMemory::new(
        File::options()
            .read(true)
            .write(true)
            .open(args.canister_state)
            .expect("canister state file must be available"),
    ));

    ic_btc_canister::init(Config {
        network: args.network,
        stability_threshold: args.stability_threshold,
        ..Config::default()
    });

    // Load large UTXOs.
    let mut p = args.canister_state_dir.clone();
    p.push("large_utxos");
    println!("reading large utxos");
    let mut bytes = vec![];
    File::open(p).unwrap().read_to_end(&mut bytes).unwrap();

    println!("done");

    let large_utxos: BTreeMap<OutPoint, (TxOut, Height)> =
        ciborium::de::from_reader(&*bytes).expect("failed to decode state");

    println!("Adding {} large utxos", large_utxos.len());

    // Read the unstable blocks.
    let unstable_blocks_file = File::open(&args.unstable_blocks).unwrap();
    let mut unstable_blocks_file = BufReader::new(unstable_blocks_file);

    let anchor_block = read_block(&mut unstable_blocks_file);
    let next_block = read_block(&mut unstable_blocks_file);

    println!(
        "Anchor block hash: {:?}",
        anchor_block.block_hash().to_string()
    );
    println!("Next block hash: {:?}", next_block.block_hash().to_string());

    println!("Ingesting unstable blocks..");
    with_state_mut(|s| {
        s.utxos.utxos.large_utxos = large_utxos;

        s.utxos.next_height = args.stable_height;

        // Ingest the blocks.
        s.unstable_blocks =
            UnstableBlocks::new(&s.utxos, args.stability_threshold as u32, anchor_block);
        unstable_blocks::push(&mut s.unstable_blocks, &s.utxos, next_block).unwrap();
    });

    println!(
        "# Small UTXOs: {}",
        with_state(|s| s.utxos.utxos.small_utxos.len())
    );
    println!(
        "# Medium UTXOs: {}",
        with_state(|s| s.utxos.utxos.medium_utxos.len())
    );
    println!("# Balances: {}", with_state(|s| s.utxos.balances_len()));
    println!(
        "# Address UTXOs: {}",
        with_state(|s| s.utxos.address_utxos_len())
    );

    println!("Running pre-upgrade..");
    pre_upgrade();
    println!("Done.");
}
