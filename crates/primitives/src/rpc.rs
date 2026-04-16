use alloy_consensus::{
    Receipt, ReceiptWithBloom, Transaction, TxReceipt, transaction::TransactionMeta,
};
use alloy_primitives::{Address, BlockHash, TxHash, TxKind};
use alloy_rpc_types_eth::Log;
use reth_optimism_primitives::{OpReceipt, OpTransactionSigned};
use reth_primitives::Recovered;
use reth_rpc_eth_types::utils::calculate_gas_used_and_next_log_index;

/// RISE transaction receipt without OP L1 fee fields (always zero on RISE).
/// Several fields like `block_number` are concrete instead of `Option`, as
/// we always have them from pending to canonical receipts.
///
/// Reference shape:
/// <https://github.com/alloy-rs/alloy/blob/v1.8.3/crates/rpc-types-eth/src/transaction/receipt.rs#L16-L69>
#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RiseRpcTransactionReceipt {
    /// The consensus receipt.
    #[serde(flatten)]
    pub inner: ReceiptWithBloom<OpReceipt<Log>>,
    /// Transaction hash.
    pub transaction_hash: TxHash,
    /// Index within the block.
    #[serde(with = "alloy_serde::quantity")]
    pub transaction_index: u64,
    /// Hash of the block this transaction was included within.
    #[serde(default)]
    pub block_hash: Option<BlockHash>,
    /// Number of the block this transaction was included within.
    #[serde(with = "alloy_serde::quantity")]
    pub block_number: u64,
    /// Gas used by this transaction alone.
    #[serde(with = "alloy_serde::quantity")]
    pub gas_used: u64,
    /// The price paid post-execution by the transaction.
    #[serde(with = "alloy_serde::quantity")]
    pub effective_gas_price: u128,
    /// Address of the sender.
    pub from: Address,
    /// Address of the receiver. `None` for contract creation.
    pub to: Option<Address>,
    /// Contract address created, or `None` if not a deployment.
    pub contract_address: Option<Address>,
}

impl RiseRpcTransactionReceipt {
    pub fn new(
        tx: &Recovered<OpTransactionSigned>,
        receipt: OpReceipt,
        meta: TransactionMeta,
        all_receipts: &[OpReceipt],
    ) -> Self {
        let from = tx.signer();

        let (contract_address, to) = match tx.kind() {
            TxKind::Create => (Some(from.create(tx.nonce())), None),
            TxKind::Call(address) => (None, Some(address)),
        };

        let (gas_used_before, next_log_index) =
            calculate_gas_used_and_next_log_index(meta.index, all_receipts);
        let gas_used = receipt
            .cumulative_gas_used()
            .saturating_sub(gas_used_before);

        let map_logs = |receipt: Receipt| Receipt {
            status: receipt.status,
            cumulative_gas_used: receipt.cumulative_gas_used,
            logs: Log::collect_for_receipt(next_log_index, meta, receipt.logs),
        };

        Self {
            inner: match receipt {
                OpReceipt::Legacy(receipt) => OpReceipt::Legacy(map_logs(receipt)),
                OpReceipt::Eip2930(receipt) => OpReceipt::Eip2930(map_logs(receipt)),
                OpReceipt::Eip1559(receipt) => OpReceipt::Eip1559(map_logs(receipt)),
                OpReceipt::Eip7702(receipt) => OpReceipt::Eip7702(map_logs(receipt)),
                OpReceipt::Deposit(receipt) => OpReceipt::Deposit(receipt.map_inner(map_logs)),
            }
            .into_with_bloom(),
            transaction_hash: meta.tx_hash,
            transaction_index: meta.index,
            block_hash: Some(meta.block_hash),
            block_number: meta.block_number,
            from,
            to,
            gas_used,
            contract_address,
            effective_gas_price: tx.effective_gas_price(meta.base_fee),
        }
    }
}
