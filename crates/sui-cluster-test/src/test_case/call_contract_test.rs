// Copyright (c) Mysten Labs, Inc.
// SPDX-License-Identifier: Apache-2.0

use async_trait::async_trait;
use jsonrpsee::rpc_params;
use move_core_types::language_storage::TypeTag;
use serde::Deserialize;
use serde_json::json;
use tracing::info;

use sui::client_commands::{EXAMPLE_NFT_DESCRIPTION, EXAMPLE_NFT_NAME, EXAMPLE_NFT_URL};
use sui_json::SuiJsonValue;
use sui_json_rpc_types::{ObjectChange, SuiEvent, SuiTransactionEffectsAPI};
use sui_types::id::ID;
use sui_types::{
    base_types::{ObjectID, SuiAddress},
    object::Owner,
    SUI_FRAMEWORK_ADDRESS, SUI_FRAMEWORK_OBJECT_ID,
};

use crate::helper::ObjectChecker;
use crate::{TestCaseImpl, TestContext};

pub struct CallContractTest;

#[async_trait]
impl TestCaseImpl for CallContractTest {
    fn name(&self) -> &'static str {
        "CallContract"
    }

    fn description(&self) -> &'static str {
        "Test calling move contract"
    }

    async fn run(&self, ctx: &mut TestContext) -> Result<(), anyhow::Error> {
        info!("Testing calling move contract.");
        let signer = ctx.get_wallet_address();
        let mut sui_objs = ctx.get_sui_from_faucet(Some(1)).await;
        let gas_obj = sui_objs.swap_remove(0);

        let args_json = json!([EXAMPLE_NFT_NAME, EXAMPLE_NFT_DESCRIPTION, EXAMPLE_NFT_URL,]);
        let mut args = vec![];
        for a in args_json.as_array().unwrap() {
            args.push(SuiJsonValue::new(a.clone()).unwrap());
        }
        let type_args: Vec<TypeTag> = vec![];
        let params = rpc_params![
            signer,
            ObjectID::from(SUI_FRAMEWORK_ADDRESS),
            "devnet_nft",
            "mint",
            type_args,
            args,
            Some(*gas_obj.id()),
            10000
        ];

        let data = ctx
            .build_transaction_remotely("sui_moveCall", params)
            .await?;
        let response = ctx.sign_and_execute(data, "call contract").await;

        // Retrieve created nft
        let nft_id = response
            .effects
            .as_ref()
            .unwrap()
            .created()
            .first()
            .expect("Failed to create NFT")
            .reference
            .object_id;

        // Examine effects
        let events = &response.events.as_ref().unwrap().data;
        let object_changes = response.object_changes.as_ref().unwrap();
        // Only one move event emitted
        assert_eq!(
            events.len(),
            1,
            "Expect 1 event emitted, but got {}",
            events.len()
        );

        let new_object_version = object_changes
            .iter()
            .find_map(|e| match e {
                ObjectChange::Created {
                    sender,
                    owner,
                    object_type,
                    object_id,
                    version,
                    ..
                } if sender == &signer
                    && owner == &Owner::AddressOwner(signer)
                    && object_type.to_string() == "0x2::devnet_nft::DevNetNFT"
                    && object_id == &nft_id =>
                {
                    Some(*version)
                }

                _ => None,
            })
            .unwrap_or_else(|| panic!("Expect such a NewObject in events {:?}", events));

        events.iter().find(|e| matches!(e, SuiEvent{
            package_id,
            sender,
            bcs,
            type_,
            ..
        } if
            package_id == &SUI_FRAMEWORK_OBJECT_ID
            && sender == &signer
            && &type_.to_string() == "0x2::devnet_nft::MintNFTEvent"
            && bcs::from_bytes::<MintNFTEvent>(bcs).unwrap() == MintNFTEvent {object_id: ID::new(nft_id), creator: signer, name: EXAMPLE_NFT_NAME.into()}
        )).unwrap_or_else(|| panic!("Expect such a MoveEvent in events {:?}", events));

        // Verify fullnode observes the txn
        ctx.let_fullnode_sync(vec![response.digest], 5).await;

        let object = ObjectChecker::new(nft_id)
            .owner(Owner::AddressOwner(signer))
            .check_into_object(ctx.get_fullnode_client())
            .await;

        assert_eq!(
            object.version, new_object_version,
            "Expect sequence number to be 1"
        );

        Ok(())
    }
}

#[derive(Deserialize, Debug, PartialEq)]
struct MintNFTEvent {
    // The Object ID of the NFT
    object_id: ID,
    // The creator of the NFT
    creator: SuiAddress,
    // The name of the NFT
    name: String,
}
