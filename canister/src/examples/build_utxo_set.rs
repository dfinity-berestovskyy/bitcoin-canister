//! A script for building the Bitcoin UTXO set.
//!
//! Example run:
//!
//! cargo run --release --example build-utxo-set -- \
//!     --state-path ./state-path \
//!     --blocks-path /home/ubuntu/.bitcoin/testnet3/blocks \
//!     --network testnet \
//!     --start-file 0 \
//!     --end-file 10 \
//!     --until-height 25000
use bitcoin::{
    blockdata::constants::genesis_block, consensus::Decodable, Block, BlockHash, BlockHeader,
    Network,
};
use byteorder::{LittleEndian, ReadBytesExt};
use clap::Parser;
use ic_btc_canister::{
    heartbeat, pre_upgrade, runtime,
    state::main_chain_height,
    state::State,
    types::{GetSuccessorsCompleteResponse, GetSuccessorsResponse, Network as IcBtcNetwork},
    with_state,
};
use ic_stable_structures::Memory;
use rusty_leveldb::{DBIterator, LdbIterator, Options, DB};
use std::{
    cmp::{max, min},
    collections::{BTreeMap, HashMap},
    fs::File,
    io::{BufReader, BufWriter, Cursor, Seek, SeekFrom},
    path::{Path, PathBuf},
    str::FromStr,
    time::SystemTime,
};

/*struct FileMemory {
    writer: BufWriter<File>,
}

impl FileMemory {
    fn new(file: File) -> Self {
        Self {
            // 1 gib capacity
            writer: BufWriter::with_capacity(1024 * 1024 * 1024, file),
        }
    }
}

impl Memory for FileMemory {
    /// Returns the current size of the stable memory in WebAssembly
    /// pages. (One WebAssembly page is 64Ki bytes.)
    fn size(&self) -> u64 {
        1024 * 1024 * 1024 * 1024 // very large size.
    }

    /// Tries to grow the memory by new_pages many pages containing
    /// zeroes.  If successful, returns the previous size of the
    /// memory (in pages).  Otherwise, returns -1.
    fn grow(&self, pages: u64) -> i64 {
        todo!(); // isn't really needed.
    }

    /// Copies the data referred to by offset out of the stable memory
    /// and replaces the corresponding bytes in dst.
    fn read(&self, offset: u64, dst: &mut [u8]) {

    }

    /// Copies the data referred to by src and replaces the
    /// corresponding segment starting at offset in the stable memory.
    fn write(&self, offset: u64, src: &[u8]);
}*/

pub trait BlockchainRead: std::io::Read {
    #[inline]
    fn read_varint(&mut self) -> usize {
        let mut n = 0;
        loop {
            let ch_data = self.read_u8();
            n = (n << 7) | (ch_data & 0x7F) as usize;
            if ch_data & 0x80 > 0 {
                n += 1;
            } else {
                break;
            }
        }
        n
    }

    #[inline]
    fn read_u8(&mut self) -> u8 {
        let mut slice = [0u8; 1];
        self.read_exact(&mut slice).unwrap();
        slice[0]
    }
}

impl BlockchainRead for Cursor<&[u8]> {}
impl BlockchainRead for Cursor<Vec<u8>> {}

#[derive(Parser, Debug)]
struct Args {
    /*/// A path to load/store the state.
    #[clap(long, value_hint = clap::ValueHint::DirPath)]
    state_path: PathBuf,*/
    /// The path to the "blocks" folder created by bitcoind.
    #[clap(long, value_hint = clap::ValueHint::DirPath)]
    blocks_path: PathBuf,

    /// The bitcoin network.
    #[clap(long)]
    network: Network,

    /// Insert blocks until the chain reaches this tip.
    #[clap(long)]
    tip: String,
}

fn build_block_index(
    path: &PathBuf,
    tip: BlockHash,
    network: Network,
) -> BTreeMap<u32, (u32, u32)> {
    let mut block_index_path = path.clone();
    block_index_path.push("blocks");
    block_index_path.push("index");

    let mut db = DB::open(block_index_path, Options::default()).unwrap();
    let genesis_blockhash =
        bitcoin::blockdata::constants::genesis_block(network.into()).block_hash();
    let mut blockhash = tip;

    let mut block_index: BTreeMap<u32, (u32, u32)> = BTreeMap::new();

    while let Some(res) = get_block_info(&mut db, &blockhash) {
        block_index.insert(res.0, (res.1, res.2));
        blockhash = res.3;
    }

    block_index
}

#[async_std::main]
async fn main() {
    let args = Args::parse();

    let mut blocks_path = args.blocks_path.clone();
    blocks_path.push("blocks");

    let tip = BlockHash::from_str(&args.tip).expect("tip must be valid.");

    println!("Building block index...");

    let block_index = build_block_index(&args.blocks_path, tip, args.network);

    println!("Initializing...");

    ic_btc_canister::init(ic_btc_canister::types::InitPayload {
        stability_threshold: 2,
        network: IcBtcNetwork::Testnet,
        blocks_source: None,
    });

    println!("Playing blocks...");

    let mut from_height = with_state(main_chain_height) + 1;
    let until_height = 30_000;
    let num_blocks_to_fetch = 1_000;

    while with_state(main_chain_height) < until_height {
        from_height = with_state(main_chain_height) + 1;
        let next_height = min(from_height + num_blocks_to_fetch, until_height + 1);

        let responses = (from_height..next_height)
            .map(|height| {
                let (file, data_pos) = block_index.get(&height).unwrap_or_else(|| {
                    panic!("height {} doesn't exist", height);
                });
                let block = read_block(&blocks_path, *file, *data_pos);

                use bitcoin::consensus::Encodable;
                let mut block_bytes = vec![];
                Block::consensus_encode(&block, &mut block_bytes).unwrap();
                GetSuccessorsResponse::Complete(GetSuccessorsCompleteResponse {
                    blocks: vec![block_bytes],
                    next: vec![],
                })
            })
            .collect();

        runtime::set_successors_responses(responses);

        // Run the heartbeat until we process all the blocks.
        let mut i = 0;
        loop {
            heartbeat().await;

            if i % 1000 == 0 {
                // The `main_chain_height` call is a bit expensive, so we only check every once
                // in a while.
                if with_state(main_chain_height)
                    == min(from_height + num_blocks_to_fetch - 1, until_height)
                {
                    break;
                }

                // TODO: actually check stable height and not main height.
            }

            i += 1;
        }
        println!("Height :{:?}", with_state(main_chain_height));
    }

    pre_upgrade();

    use std::fs::File;
    use std::io::prelude::*;
    use std::path::Path;
    println!(
        "memory size: {:?}",
        ic_btc_canister::memory::MEMORY.with(|m| m.borrow().len())
    );

    let path = Path::new("testnet_stable_memory.bin");
    let display = path.display();

    let mut file = match File::create(&path) {
        Err(why) => panic!("couldn't create {}: {}", display, why),
        Ok(file) => file,
    };

    ic_btc_canister::memory::MEMORY.with(|m| match file.write_all(&m.borrow()) {
        Err(why) => panic!("couldn't write to {}: {}", display, why),
        Ok(_) => println!("successfully wrote to {}", display),
    });
}

fn get_block_info(db: &mut DB, block_hash: &BlockHash) -> Option<(u32, u32, u32, BlockHash)> {
    use std::convert::TryInto;
    let mut key: Vec<u8> = vec![98];
    key.extend(block_hash.to_vec());

    let value = db.get(&key).unwrap();

    let mut reader = Cursor::new(value);

    let _version = reader.read_varint() as i32;
    let height = reader.read_varint() as u32;
    let _status = reader.read_varint() as u32;

    let _tx = reader.read_varint() as u32;
    let file = reader.read_varint() as i32;
    let data_pos = reader.read_varint() as u32;
    let _undo_pos = reader.read_varint() as u32;

    match BlockHeader::consensus_decode(&mut reader) {
        Err(_) => None,
        Ok(header) => Some((height, file as u32, data_pos, header.prev_blockhash)),
    }
}

fn read_block(block_path: &PathBuf, file: u32, data_pos: u32) -> Block {
    let mut blk_file = File::open(block_path.join(format!("blk{:0>5}.dat", file))).unwrap();

    blk_file.seek(SeekFrom::Start(data_pos as u64)).unwrap();

    Block::consensus_decode(&mut blk_file).unwrap()
}

/*fn main() {
    // load memory.
    // Create a path to the desired file
    let path = std::path::Path::new("testnet_stable_memory.bin");
    use std::io::Read;
    let display = path.display();

    // Open the path in read-only mode, returns `io::Result<File>`
    let mut file = match File::open(&path) {
        Err(why) => panic!("couldn't open {}: {}", display, why),
        Ok(file) => file,
    };

    // Read the file contents into a string, returns `io::Result<usize>`
    ic_btc_canister::memory::MEMORY.with(|m| match file.read_to_end(&mut m.borrow_mut()) {
        Err(why) => panic!("couldn't read {}: {}", display, why),
        Ok(_) => print!("loaded"),
    });

    ic_btc_canister::memory::MEMORY.with(|m| {
        println!("loaded {} bytes in memory", m.borrow().len());
    });

    ic_btc_canister::post_upgrade();

    // Validate we've ingested all the blocks.
    assert_eq!(with_state(main_chain_height), 30_000);
}*/
