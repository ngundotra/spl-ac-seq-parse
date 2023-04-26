// Fetch a transaction from Solana mainnet RPC and parse it
use reqwest::Client;
use tokio;

use anchor_client::anchor_lang::AnchorDeserialize;

use serde::{Deserialize, Serialize};
use spl_account_compression::{AccountCompressionEvent, ChangeLogEvent};
use std::error::Error;

#[derive(Serialize)]
struct TransactionRequest<'a> {
    jsonrpc: &'a str,
    id: u64,
    method: &'a str,
    params: [&'a str; 2],
}

// JSON-RPC response payload
#[allow(dead_code, non_snake_case)]
#[derive(Deserialize)]
struct TransactionResponse {
    jsonrpc: String,
    id: u64,
    result: Option<TransactionResult>,
}

// Transaction result structure
#[derive(Deserialize)]
struct TransactionResult {
    meta: TransactionMeta,
    transaction: Transaction,
}

// Transaction metadata
#[allow(non_snake_case)]
#[derive(Deserialize)]
struct TransactionMeta {
    innerInstructions: Vec<InnerInstruction>,
    logMessages: Option<Vec<String>>,
}

// InnerInstruction structure
#[derive(Deserialize)]
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

#[tokio::main]
async fn main() -> Result<(), Box<dyn Error>> {
    let transaction_id =
        "UvEwd9MoVjfgBJeee1UC51K7eacMzoeq4MRqyYiUfcPY6Jn4UszRNuYkWAkLv15MchSKLn2z7TzhXBuTZMj47Ra";
    let url = "https://api.mainnet-beta.solana.com";
    let client = Client::new();

    let arg: &str = "json";

    let request_payload = TransactionRequest {
        jsonrpc: "2.0",
        id: 1,
        method: "getTransaction",
        params: [&transaction_id, arg],
    };

    let response = client
        .post(url)
        .json(&request_payload)
        .send()
        .await?
        .json::<TransactionResponse>()
        .await?;

    println!("Tx: {}", transaction_id);

    if let Some(result) = response.result {
        let account_keys = &result.transaction.message.accountKeys;

        for (i, _) in result.transaction.message.instructions.iter().enumerate() {
            if let Some(inner_ixs) = result.meta.innerInstructions.get(i) {
                for (_, inner_ix) in inner_ixs.instructions.iter().enumerate() {
                    let program = account_keys.get(inner_ix.programIdIndex as usize).unwrap();

                    // Parse noop event
                    if program.to_string() == spl_noop::id().to_string() {
                        let data = bs58::decode(&inner_ix.data).into_vec()?;

                        if let Ok(event) = &AccountCompressionEvent::try_from_slice(&data) {
                            if let AccountCompressionEvent::ChangeLog(_cl_data) = event {
                                let ChangeLogEvent::V1(cl_data) = _cl_data;
                                println!("ChangeLogEvent: {:?}", cl_data.seq);
                            }
                        }
                    }
                }
            }
        }
    }
    Ok(())
}
