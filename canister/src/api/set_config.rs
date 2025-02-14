use crate::SetConfigRequest;
use std::convert::TryInto;

pub fn set_config(request: SetConfigRequest) {
    verify_caller();

    crate::with_state_mut(|s| {
        if let Some(syncing) = request.syncing {
            s.syncing_state.syncing = syncing;
        }

        if let Some(fees) = request.fees {
            s.fees = fees;
        }

        if let Some(stability_threshold) = request.stability_threshold {
            s.unstable_blocks.set_stability_threshold(
                stability_threshold
                    .try_into()
                    .expect("stability threshold too large"),
            );
        }
    });
}

fn verify_caller() {
    #[cfg(target_arch = "wasm32")]
    {
        use ic_cdk::export::Principal;
        use std::str::FromStr;

        // TODO(EXC-1279): Instead of hard-coding a principal, check that the caller is a canister controller.
        if ic_cdk::api::caller()
            != Principal::from_str(
                "5kqj4-ymytp-ozksm-u62pb-po22y-zqqzf-2o4th-5shdt-m5j6r-kgyfi-2qe",
            )
            .unwrap()
        {
            panic!("Unauthorized sender");
        }
    }
}

#[cfg(test)]
mod test {
    use super::*;
    use crate::{
        init,
        types::{Config, Fees, Flag},
        with_state,
    };
    use proptest::prelude::*;

    #[test]
    fn set_stability_threshold() {
        init(Config::default());

        proptest!(|(
            stability_threshold in 0..150u128,
        )| {
            set_config(SetConfigRequest {
                stability_threshold: Some(stability_threshold),
                ..Default::default()
            });

            assert_eq!(
                with_state(|s| s.unstable_blocks.stability_threshold()),
                stability_threshold as u32
            );
        });
    }

    #[test]
    fn set_syncing() {
        init(Config::default());

        for flag in &[Flag::Enabled, Flag::Disabled] {
            set_config(SetConfigRequest {
                syncing: Some(*flag),
                ..Default::default()
            });

            assert_eq!(
                with_state(|s| s.syncing_state.syncing),
                *flag
            );
        }
    }

    #[test]
    fn set_fees() {
        init(Config::default());

        proptest!(|(
            get_utxos in 0..1_000_000_000_000u128,
            get_balance in 0..1_000_000_000_000u128,
            get_current_fee_percentiles in 0..1_000_000_000_000u128,
            send_transaction_base in 0..1_000_000_000_000u128,
            send_transaction_per_byte in 0..1_000_000_000_000u128,
        )| {
            let fees = Fees {
                get_utxos,
                get_balance,
                get_current_fee_percentiles,
                send_transaction_base,
                send_transaction_per_byte
            };

            set_config(SetConfigRequest {
                fees: Some(fees.clone()),
                ..Default::default()
            });

            with_state(|s| assert_eq!(s.fees, fees));
        });
    }
}
