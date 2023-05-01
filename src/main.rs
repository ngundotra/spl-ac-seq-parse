// Fetch a transaction from Solana mainnet RPC and parse it
use async_recursion::async_recursion;
use reqwest::Client;
use serde_json::Value;
use solana_client::rpc_client::RpcClient;
use solana_client::rpc_config::EncodingConfig;
use solana_sdk::commitment_config::CommitmentConfig;
use solana_sdk::pubkey;
use solana_sdk::pubkey::Pubkey;
use solana_sdk::signature::Signature;
use solana_sdk::transaction::VersionedTransaction;
use solana_transaction_status::option_serializer::OptionSerializer;
use solana_transaction_status::EncodedConfirmedTransactionWithStatusMeta;
use solana_transaction_status::EncodedTransaction;
use solana_transaction_status::UiMessage;
use solana_transaction_status::UiParsedMessage;
use solana_transaction_status::UiTransactionEncoding;
use solana_transaction_status::UiTransactionStatusMeta;
use tokio;

use anchor_client::anchor_lang::AnchorDeserialize;

use serde::{Deserialize, Serialize};
use spl_account_compression::{AccountCompressionEvent, ChangeLogEvent};
use std::{error::Error, str::FromStr};

#[derive(Serialize)]
struct TransactionRequest<'a> {
    jsonrpc: &'a str,
    id: u64,
    method: &'a str,
    params: [&'a str; 2],
}

// JSON-RPC response payload
#[allow(dead_code, non_snake_case)]
#[derive(Deserialize, Debug)]
struct TransactionResponse {
    jsonrpc: String,
    id: u64,
    result: Option<TransactionResult>,
}

// Transaction result structure
#[derive(Deserialize, Debug)]
struct TransactionResult {
    meta: TransactionMeta,
    transaction: Transaction,
}

// Transaction metadata
#[allow(non_snake_case)]
#[derive(Deserialize, Debug)]
struct TransactionMeta {
    innerInstructions: Vec<InnerInstruction>,
    logMessages: Option<Vec<String>>,
}

// InnerInstruction structure
#[derive(Deserialize, Debug)]
struct InnerInstruction {
    index: u64,
    instructions: Vec<Instruction>,
}

// Instruction structure
#[allow(non_snake_case)]
#[derive(Debug, Deserialize)]
struct Instruction {
    programIdIndex: u64,
    accounts: Vec<u64>,
    data: String,
}

// Transaction structure
#[derive(Deserialize, Debug)]
struct Transaction {
    message: TransactionMessage,
}

// Transaction message structure
#[allow(dead_code, non_snake_case)]
#[derive(Debug, Deserialize)]
struct TransactionMessage {
    accountKeys: Vec<String>,
    header: Header,
    recentBlockhash: String,
    instructions: Vec<Instruction>,
    addressTableLookups: Option<Vec<AddressTableLookup>>,
}

#[allow(dead_code, non_snake_case)]
#[derive(Debug, Deserialize)]
struct Header {
    numRequiredSignatures: u64,
    numReadonlySignedAccounts: u64,
    numReadonlyUnsignedAccounts: u64,
}

#[allow(dead_code, non_snake_case)]
#[derive(Debug, Deserialize)]
struct AddressTableLookup {
    accountKey: String,
    writableIndexes: Vec<u64>,
    readonlyIndexes: Vec<u64>,
}

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum TransactionParsingError {
    #[error("Meta parsing error: {0}")]
    MetaError(String),
    #[error("Transaction decoding error: {0}")]
    DecodingError(String),
}

#[async_recursion]
pub async fn process_txn(sig_str: &str, client: &RpcClient, retries: u8) {
    println!("Tagging: {}", sig_str);
    let sig = Signature::from_str(sig_str).unwrap();
    let tx = client.get_transaction_with_config(
        &sig,
        solana_client::rpc_config::RpcTransactionConfig {
            encoding: Some(UiTransactionEncoding::Base58),
            commitment: Some(CommitmentConfig::confirmed()),
            max_supported_transaction_version: Some(0),
        },
    );
    // .unwrap();

    match tx {
        Ok(txn) => {
            println!("Homeboy is fire");
            let seq_numbers = parse_txn_sequence(&txn).await;
            println!("Homeboy is ok? {}", seq_numbers.is_ok());
            if let Ok(arr) = seq_numbers {
                println!("Homeboy got {:?}", arr);
                for seq in arr {
                    println!("{} {}", seq, sig);
                }
            }
        }
        Err(e) => {
            println!("Homeboy is fucked up");
            if retries > 0 {
                eprintln!("Retrying transaction {} retry no {}: {}", sig, retries, e);
                process_txn(sig_str, &client, retries - 1).await;
            } else {
                eprintln!("Could not load transaction {}: {}", sig, e);
            }
        }
    }
}

// Parse the trasnaction data
pub async fn parse_txn_sequence(
    txn: &EncodedConfirmedTransactionWithStatusMeta,
) -> Result<Vec<u64>, TransactionParsingError> {
    let mut seq_updates = vec![];

    // Get `UiTransaction` out of `EncodedTransactionWithStatusMeta`.
    let meta: UiTransactionStatusMeta =
        txn.transaction
            .meta
            .clone()
            .ok_or(TransactionParsingError::MetaError(String::from(
                "couldn't load meta",
            )))?;

    let transaction: VersionedTransaction =
        txn.transaction
            .transaction
            .decode()
            .ok_or(TransactionParsingError::DecodingError(String::from(
                "Couldn't parse transction",
            )))?;

    let msg = transaction.message;
    if let OptionSerializer::Some(loaded_addresses) = meta.loaded_addresses {
        let mut account_keys = msg.static_account_keys().to_vec();

        // Add the account lookup stuff
        loaded_addresses.writable.iter().for_each(|pkey| {
            account_keys.push(Pubkey::from_str(pkey).unwrap());
        });
        loaded_addresses.readonly.iter().for_each(|pkey| {
            account_keys.push(Pubkey::from_str(pkey).unwrap());
        });
        println!("Account keys len: {:?}", account_keys.len());

        // See https://github.com/ngundotra/spl-ac-seq-parse/blob/main/src/main.rs
        if let OptionSerializer::Some(inner_instructions_vec) = meta.inner_instructions.as_ref() {
            for inner_ixs in inner_instructions_vec.iter() {
                for (_, inner_ix) in inner_ixs.instructions.iter().enumerate() {
                    if let solana_transaction_status::UiInstruction::Compiled(instr) = inner_ix {
                        println!("Program: {:?}", instr.program_id_index);
                        if let Some(program) = account_keys.get(instr.program_id_index as usize) {
                            if program.to_string() == spl_noop::id().to_string() {
                                let data = bs58::decode(&instr.data).into_vec().map_err(|_| {
                                    TransactionParsingError::DecodingError(String::from(
                                        "error base58ing",
                                    ))
                                })?;
                                if let Ok(event) = &AccountCompressionEvent::try_from_slice(&data) {
                                    if let AccountCompressionEvent::ChangeLog(_cl_data) = event {
                                        let ChangeLogEvent::V1(cl_data) = _cl_data;
                                        println!("{}: {}", cl_data.id, cl_data.seq);
                                        seq_updates.push(cl_data.seq);
                                    }
                                }
                            }
                        } else {
                            println!("Program not found for index: {}", instr.program_id_index);
                        }
                    }
                }
            }
        }
    }

    Ok(seq_updates)
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn Error>> {
    let transaction_id =
        "3JobYeWP3xJ3Fb2rJG2GRtp9A55kTjiwj5qbmW6AsWGFWcWuA6BkG4m9ZRZ9rDQX7AynWruZdjQeuunf7ySh76Kh";
    let url = "https://api.mainnet-beta.solana.com";

    let rpc = RpcClient::new(url.to_string());

    let sig = solana_sdk::signature::Signature::from_str(transaction_id.as_ref()).unwrap();

    process_txn(&transaction_id, &rpc, 3).await;
    // let response = rpc
    //     // .get_transaction(&sig, solana_transaction_status::UiTransactionEncoding::Json)
    //     .get_transaction_with_config(&sig, config)
    //     .unwrap();

    // println!("Resposne: {:?}", response);

    Ok(())
}
// let url = "https://rpc.helius.xyz/?api-key=8ff76c55-268e-4c41-9916-cb55b8132089";
//     let client = Client::new();

//     let arg: &str = "json";

//     let request_payload = TransactionRequest {
//         jsonrpc: "2.0",
//         id: 1,
//         method: "getTransaction",
//         params: [&transaction_id, arg],
//     };

//     let response = client
//         .post(url)
//         .json(&request_payload)
//         .send()
//         .await?
//         .json::<TransactionResponse>()
//         .await?;

//     println!("Tx: {} {:?}", transaction_id, response);

//     if let Some(result) = response.result {
//         let account_keys = &result.transaction.message.accountKeys;

//         for (i, outer_ix) in result.transaction.message.instructions.iter().enumerate() {
//             let outer_program = account_keys.get(outer_ix.programIdIndex as usize).unwrap();
//             println!("Outer program: {}, {}", i, outer_program);

//             for inner_ixs in result.meta.innerInstructions.iter() {
//                 if inner_ixs.index != i as u64 {
//                     continue;
//                 }

//                 for (_, inner_ix) in inner_ixs.instructions.iter().enumerate() {
//                     let program = account_keys.get(inner_ix.programIdIndex as usize).unwrap();
//                     println!("Program: {} {:?}", program, inner_ix);

//                     // Parse noop event
//                     if program.to_string() == spl_noop::id().to_string() {
//                         let data = bs58::decode(&inner_ix.data).into_vec()?;

//                         if let Ok(event) = &AccountCompressionEvent::try_from_slice(&data) {
//                             if let AccountCompressionEvent::ChangeLog(_cl_data) = event {
//                                 let ChangeLogEvent::V1(cl_data) = _cl_data;
//                                 println!("ChangeLogEvent: {:?}", cl_data.seq);
//                             }
//                         }
//                     }
//                 }
//             }
//             // if let Some(inner_ixs) = result.meta.innerInstructions.get(i) {
//         }
//     }
//     Ok(())
// }
