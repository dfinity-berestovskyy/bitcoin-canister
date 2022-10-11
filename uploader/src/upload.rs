use candid::{encode_args, CandidType, Decode, Encode, Nat};
use ic_agent::{export::Principal, Agent};
use serde::Deserialize;
use std::{
    fs::File,
    io::{BufReader, Read, Seek, SeekFrom},
    str::FromStr,
};

#[derive(CandidType)]
struct Empty;

/*async fn create_a_canister() -> Result<Principal, Box<dyn std::error::Error>> {
    let agent = Agent::builder()
        .with_url(URL)
        .with_identity(create_identity())
        .build()?;
    // Only do the following call when not contacting the IC main net (e.g. a local emulator).
    // This is important as the main net public key is static and a rogue network could return
    // a different key.
    // If you know the root key ahead of time, you can use `agent.set_root_key(root_key)?;`.
    agent.fetch_root_key().await?;
    let management_canister_id = Principal::from_text("aaaaa-aa")?;


    // Create a call to the management canister to create a new canister ID,
    // and wait for a result.
    let response = agent
        .update(
            &management_canister_id,
            "provisional_create_canister_with_cycles",
        )
        .with_arg(&Encode!(&Argument { amount: None })?)
        .call_and_wait(waiter)
        .await?;

    let result = Decode!(response.as_slice(), CreateCanisterResult)?;
    let canister_id: Principal = result.canister_id;
    Ok(canister_id)
}*/

async fn upload(agent: &Agent, canister_id: &Principal, page_start: u64, bytes: &[u8]) {
    let waiter = garcon::Delay::builder()
        .throttle(std::time::Duration::from_millis(500))
        .timeout(std::time::Duration::from_secs(60 * 5))
        .build();

    let response: Vec<u8> = agent
        .update(canister_id, "write")
        .with_arg(encode_args((page_start, bytes.to_vec())).unwrap())
        .call_and_wait(waiter)
        .await
        .unwrap();
}

#[async_std::main]
async fn main() {
    let f = File::open("testnet_stable_memory.bin").unwrap();
    let mut reader = BufReader::new(f);

    // Send some bytes to the canister.
    let agent = Agent::builder()
        .with_url("http://127.0.0.1:8000/")
        //.with_identity(create_identity())
        .build()
        .expect("agent creation must succeed");

    // Only do the following call when not contacting the IC main net (e.g. a local emulator).
    // This is important as the main net public key is static and a rogue network could return
    // a different key.
    // If you know the root key ahead of time, you can use `agent.set_root_key(root_key)?;`.
    agent.fetch_root_key().await.expect("fetch root key failed");

    let canister_id =
        Principal::from_str("rrkah-fqaaa-aaaaa-aaaaq-cai").expect("invalid canister id");

    let waiter = garcon::Delay::builder()
        .throttle(std::time::Duration::from_millis(500))
        .timeout(std::time::Duration::from_secs(60 * 5))
        .build();

    let response: Vec<u8> = agent
        .query(&canister_id, "get_missing_ranges")
        .with_arg(Encode!(&Empty).unwrap())
        .call()
        .await
        .unwrap();

    let missing_pages = Decode!(&response, Vec<u64>).unwrap();

    println!("response: {:?}", missing_pages);

    // TODO: only upload missing pages.

    let mut offset = 4092;
    let mut buf = vec![0; 64 * 1024 * 31]; // 31 pages.
    reader.seek(SeekFrom::Start(4092 * 64 * 1024));
    while let Ok(bytes_read) = reader.read(&mut buf) {
        if bytes_read == 0 {
            break;
        }
        println!("uploading pages at {}", offset);
        upload(&agent, &canister_id, offset, &buf[..bytes_read]).await;
        offset += 31;
    }
}
