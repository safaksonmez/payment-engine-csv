# Payments Engine

## Overview

`payments-engine` reads a transaction CSV and writes the final balance and lock
status for each client. It handles deposits, withdrawals, disputes, resolves,
and chargebacks.

## Build and Run

```sh
cargo build
cargo run -- transactions.csv > accounts.csv
```

Pass the input file path as the sole argument. The input header is
`type,client,tx,amount`. Client IDs are unsigned 16-bit integers, and transaction
IDs are unsigned 32-bit integers. The input must supply globally unique deposit
and withdrawal IDs. Disputes, resolves, and chargebacks reuse the referenced ID
and leave `amount` empty or omit the final field. The reader trims surrounding
whitespace from fields. Amounts can have up to four decimal places.

```csv
type,client,tx,amount
deposit,7,40,12.3750
deposit,7,12,4.2500
withdrawal,7,8,3.1250
dispute,7,12,
```

The example produces the following CSV on stdout. Each client gets one row,
and row order is unspecified.

```csv
client,available,held,total,locked
7,9.2500,4.2500,13.5000,false
```

## Design

The engine processes transactions in file order with one reusable CSV record
buffer. It retains two `HashMap`s in memory.

- Accounts are keyed by client ID and store available funds, held funds, and a
  lock flag. An account is created on the client's first valid row. Total is
  derived as available plus held.
- Accepted deposits are keyed by transaction ID and store the client ID, amount,
  and dispute status. This history allows a later row to reference an earlier
  deposit. Memory grows with the number of clients and retained deposits.

Amounts use signed `i64` integers in units of `0.0001`. The engine parses decimal
text and writes four decimal places without floating-point arithmetic. Checked
arithmetic detects overflow.

A deposit credits available funds, and a withdrawal debits them only when
sufficient funds are available. A dispute moves funds from available to held,
and a resolve reverses that transfer. Both preserve total. A chargeback removes
the disputed funds from held, reduces total, and locks the account.

## Assumptions and Limitations

- **Dispute eligibility.** Only accepted deposits can be disputed. The deposit
  must belong to the client on the reference row. Unknown or cross-client
  references, repeated active disputes, and resolves or chargebacks without an
  active dispute leave balances unchanged. A resolved deposit can be disputed again.
- **Negative balances.** A dispute holds the full deposit amount even if some
  of it has been withdrawn. Available funds can become negative, and a later
  chargeback can make total negative. Deposit and withdrawal amounts must be
  nonnegative.
- **Permanent lock.** The required chargeback lock is treated as permanent.
  Later valid transactions for that client are ignored, including resolutions
  of other outstanding disputes. Input validation still applies to those rows.
- **Numeric range.** The fixed-point representation spans
  `-922337203685477.5808` through `922337203685477.5807`. Amounts and balance
  changes outside the supported range fail the run.
- **CSV errors.** The `csv` crate can recover from some malformed quoting. For
  example, an amount field of `"1"2` can be read as `12`. Detected input and
  validation errors produce a diagnostic on stderr, a nonzero exit status, and
  empty stdout because the engine writes results after processing the input.

## Testing

```sh
cargo test
```

Unit tests cover deposits, successful and insufficient-funds withdrawals,
unordered IDs across clients, dispute and resolve transitions, invalid and
cross-client references, negative available balances, and permanent locking.
CLI tests cover whitespace and four-decimal output, overflow with empty stdout,
and the final CSV for a chargeback that leaves a negative total.

## AI Assistance

OpenAI Codex was used extensively for implementation, test generation, code
review, and documentation. The full AI interaction transcript will be provided
separately.
