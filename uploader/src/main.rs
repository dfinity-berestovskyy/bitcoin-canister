use ic_cdk::api::stable;
use ic_cdk_macros::{init, query, update};
use std::{cell::RefCell, cmp::min, collections::BTreeSet};

const PAGE_SIZE: u64 = 64 * 1024;

thread_local! {
    static MISSING_RANGES: RefCell<BTreeSet<u64>> = RefCell::new(BTreeSet::new());
}

#[init]
fn init(initial_size: u64) {
    stable::stable64_grow(initial_size).expect("cannot grow stabe memory");

    MISSING_RANGES.with(|mr| mr.replace((0..initial_size).step_by(31).collect()));
}

#[update]
fn write(page_start: u64, bytes: Vec<u8>) {
    // TODO: check if controller
    // TODO: check overflow?

    if !MISSING_RANGES.with(|mr| mr.borrow().contains(&page_start)) {
        panic!("invalid range");
    }

    let expected_end_page = min(page_start + 31, stable::stable64_size());

    let expected_bytes_length = ((expected_end_page - page_start) * PAGE_SIZE) as usize;

    if expected_bytes_length != bytes.len() {
        panic!(
            "expected bytes to be {} bytes but found {} bytes",
            expected_bytes_length,
            bytes.len()
        );
    }

    let offset = page_start * PAGE_SIZE;

    // Write bytes of 31 pages.
    stable::stable64_write(offset, &bytes);

    MISSING_RANGES.with(|mr| mr.borrow_mut().remove(&page_start));
}

// Returns the first 100 missing ranges.
#[query]
fn get_missing_ranges() -> Vec<u64> {
    MISSING_RANGES.with(|mr| mr.borrow().iter().take(100).cloned().collect())
}

fn main() {}
