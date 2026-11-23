use std::collections::HashMap;
use std::error::Error;
use std::io;
use std::process::ExitCode;

use csv::{ReaderBuilder, StringRecord, Trim, Writer};

const BALANCE_OVERFLOW: &str = "balance exceeds the i64 fixed-point range";

#[derive(Default)]
struct Account {
    available: i64,
    held: i64,
    locked: bool,
}

struct Deposit {
    client: u16,
    amount: i64,
    disputed: bool,
}

enum Transaction {
    Deposit(i64),
    Withdrawal(i64),
    Dispute,
    Resolve,
    Chargeback,
}

fn main() -> ExitCode {
    match run() {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("{error}");
            ExitCode::FAILURE
        }
    }
}

fn run() -> Result<(), Box<dyn Error>> {
    const USAGE: &str = "usage: cargo run -- transactions.csv > accounts.csv";

    let mut args = std::env::args_os().skip(1);
    let path = args.next().ok_or(USAGE)?;
    if args.next().is_some() {
        return Err(USAGE.into());
    }

    let mut input = ReaderBuilder::new()
        .trim(Trim::All)
        .flexible(true)
        .from_path(path)?;
    if !input
        .headers()?
        .iter()
        .eq(["type", "client", "tx", "amount"])
    {
        return Err("expected CSV header: type,client,tx,amount".into());
    }

    let mut accounts = HashMap::new();
    let mut deposits = HashMap::new();
    let mut record = StringRecord::new();
    while input.read_record(&mut record)? {
        if let Err(error) = process_record(&record, &mut accounts, &mut deposits) {
            let line = record.position().map_or(1, csv::Position::line);
            return Err(format!("CSV line {line}: {error}").into());
        }
    }

    let mut output = Writer::from_writer(io::stdout().lock());
    output.write_record(["client", "available", "held", "total", "locked"])?;
    for (client, account) in accounts {
        let total = account
            .available
            .checked_add(account.held)
            .ok_or(BALANCE_OVERFLOW)?;
        output.write_record([
            client.to_string(),
            format_amount(account.available),
            format_amount(account.held),
            format_amount(total),
            account.locked.to_string(),
        ])?;
    }
    output.flush()?;
    Ok(())
}

fn process_record(
    record: &StringRecord,
    accounts: &mut HashMap<u16, Account>,
    deposits: &mut HashMap<u32, Deposit>,
) -> Result<(), &'static str> {
    if !(3..=4).contains(&record.len()) {
        return Err("expected three or four fields");
    }
    let client = record[1].parse::<u16>().map_err(|_| "invalid client ID")?;
    let tx = record[2]
        .parse::<u32>()
        .map_err(|_| "invalid transaction ID")?;
    let amount = record.get(3).unwrap_or_default();
    let transaction = match &record[0] {
        "deposit" => Transaction::Deposit(parse_amount(amount)?),
        "withdrawal" => Transaction::Withdrawal(parse_amount(amount)?),
        "dispute" if amount.is_empty() => Transaction::Dispute,
        "resolve" if amount.is_empty() => Transaction::Resolve,
        "chargeback" if amount.is_empty() => Transaction::Chargeback,
        "dispute" | "resolve" | "chargeback" => {
            return Err("reference transaction specifies an amount");
        }
        _ => return Err("unknown transaction type"),
    };

    let account = accounts.entry(client).or_default();
    // Assume a chargeback permanently freezes all activity on the account.
    if account.locked {
        return Ok(());
    }

    match transaction {
        Transaction::Deposit(amount) => {
            let available = account
                .available
                .checked_add(amount)
                .ok_or(BALANCE_OVERFLOW)?;
            available
                .checked_add(account.held)
                .ok_or(BALANCE_OVERFLOW)?;
            account.available = available;
            deposits.insert(
                tx,
                Deposit {
                    client,
                    amount,
                    disputed: false,
                },
            );
        }
        Transaction::Withdrawal(amount) => {
            if account.available >= amount {
                account.available -= amount;
            }
        }
        Transaction::Dispute | Transaction::Resolve | Transaction::Chargeback => {
            let Some(deposit) = deposits
                .get_mut(&tx)
                .filter(|deposit| deposit.client == client)
            else {
                return Ok(());
            };
            let amount = deposit.amount;
            match transaction {
                Transaction::Dispute if !deposit.disputed => {
                    let available = account
                        .available
                        .checked_sub(amount)
                        .ok_or(BALANCE_OVERFLOW)?;
                    let held = account.held.checked_add(amount).ok_or(BALANCE_OVERFLOW)?;
                    account.available = available;
                    account.held = held;
                    deposit.disputed = true;
                }
                Transaction::Resolve if deposit.disputed => {
                    let available = account
                        .available
                        .checked_add(amount)
                        .ok_or(BALANCE_OVERFLOW)?;
                    let held = account.held.checked_sub(amount).ok_or(BALANCE_OVERFLOW)?;
                    account.available = available;
                    account.held = held;
                    deposit.disputed = false;
                }
                Transaction::Chargeback if deposit.disputed => {
                    account.held = account.held.checked_sub(amount).ok_or(BALANCE_OVERFLOW)?;
                    account.locked = true;
                    deposits.remove(&tx);
                }
                _ => {}
            }
        }
    }
    Ok(())
}

fn parse_amount(value: &str) -> Result<i64, &'static str> {
    const INVALID_AMOUNT: &str = "invalid or out-of-range amount";

    let value = value.strip_prefix('+').unwrap_or(value);
    let (whole, fraction) = value.split_once('.').unwrap_or((value, ""));
    if (whole.is_empty() && fraction.is_empty()) || fraction.len() > 4 {
        return Err(INVALID_AMOUNT);
    }

    let mut digits = whole.bytes().chain(fraction.bytes());
    let amount = digits.try_fold(0_i64, |amount, digit| {
        if !digit.is_ascii_digit() {
            return None;
        }
        amount.checked_mul(10)?.checked_add(i64::from(digit - b'0'))
    });
    let padding = 10_i64.pow(4 - fraction.len() as u32);
    amount
        .and_then(|amount| amount.checked_mul(padding))
        .ok_or(INVALID_AMOUNT)
}

fn format_amount(amount: i64) -> String {
    let sign = if amount < 0 { "-" } else { "" };
    let magnitude = amount.unsigned_abs();
    format!("{sign}{}.{:04}", magnitude / 10_000, magnitude % 10_000)
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;

    use csv::ReaderBuilder;

    use super::{Account, Deposit, process_record};

    #[derive(Default)]
    struct Ledger {
        accounts: HashMap<u16, Account>,
        deposits: HashMap<u32, Deposit>,
    }

    impl Ledger {
        fn apply(&mut self, rows: &str) {
            let mut reader = ReaderBuilder::new()
                .has_headers(false)
                .flexible(true)
                .from_reader(rows.as_bytes());
            for row in reader.records() {
                let row = row.expect("valid test CSV");
                process_record(&row, &mut self.accounts, &mut self.deposits)
                    .unwrap_or_else(|error| panic!("failed to apply {row:?}: {error}"));
            }
        }

        fn assert_balance(&self, client: u16, expected: (i64, i64, i64, bool)) {
            let account = &self.accounts[&client];
            assert_eq!(
                (
                    account.available,
                    account.held,
                    account.available + account.held,
                    account.locked,
                ),
                expected,
                "client {client} (available, held, total, locked)",
            );
        }
    }

    #[test]
    fn deposit_credits_and_successful_withdrawal_debits() {
        let mut ledger = Ledger::default();
        ledger.apply("deposit,1,1,10.5");
        ledger.assert_balance(1, (105_000, 0, 105_000, false));
        ledger.apply("withdrawal,1,2,3.25");
        ledger.assert_balance(1, (72_500, 0, 72_500, false));
    }

    #[test]
    fn processes_multiple_clients_in_file_order() {
        let mut ledger = Ledger::default();
        ledger.apply("deposit,2,90,8\ndeposit,1,70,3\nwithdrawal,2,20,2\nwithdrawal,1,10,1");
        ledger.assert_balance(1, (20_000, 0, 20_000, false));
        ledger.assert_balance(2, (60_000, 0, 60_000, false));
    }

    #[test]
    fn dispute_holds_funds_until_resolve() {
        let mut ledger = Ledger::default();
        ledger.apply("deposit,1,8,7.25\ndeposit,1,4,2.5");
        ledger.assert_balance(1, (97_500, 0, 97_500, false));
        ledger.apply("dispute,1,4,");
        ledger.assert_balance(1, (72_500, 25_000, 97_500, false));
        ledger.apply("withdrawal,1,3,8");
        ledger.assert_balance(1, (72_500, 25_000, 97_500, false));
        ledger.apply("resolve,1,4,");
        ledger.assert_balance(1, (97_500, 0, 97_500, false));
    }

    #[test]
    fn chargeback_removes_held_funds_and_permanently_locks_account() {
        let mut ledger = Ledger::default();
        ledger.apply("deposit,1,1,2\ndeposit,1,2,3\ndeposit,1,3,5\ndispute,1,1\ndispute,1,2");
        ledger.assert_balance(1, (50_000, 50_000, 100_000, false));
        ledger.apply("chargeback,1,2");
        ledger.assert_balance(1, (50_000, 20_000, 70_000, true));

        for row in [
            "deposit,1,4,1.5",
            "withdrawal,1,5,2",
            "resolve,1,1",
            "dispute,1,3",
            "chargeback,1,1",
        ] {
            ledger.apply(row);
            ledger.assert_balance(1, (50_000, 20_000, 70_000, true));
        }
    }

    #[test]
    fn ignores_unknown_transaction_references() {
        let mut ledger = Ledger::default();
        ledger.apply("deposit,1,10,4");
        for row in ["dispute,1,99", "resolve,1,99", "chargeback,1,99"] {
            ledger.apply(row);
            ledger.assert_balance(1, (40_000, 0, 40_000, false));
        }
    }

    #[test]
    fn ignores_duplicate_disputes_and_actions_without_an_active_dispute() {
        let mut ledger = Ledger::default();
        ledger.apply("deposit,1,10,4");
        for row in ["resolve,1,10", "chargeback,1,10"] {
            ledger.apply(row);
            ledger.assert_balance(1, (40_000, 0, 40_000, false));
        }

        ledger.apply("dispute,1,10");
        ledger.assert_balance(1, (0, 40_000, 40_000, false));
        ledger.apply("dispute,1,10");
        ledger.assert_balance(1, (0, 40_000, 40_000, false));

        ledger.apply("resolve,1,10");
        for row in ["resolve,1,10", "chargeback,1,10"] {
            ledger.apply(row);
            ledger.assert_balance(1, (40_000, 0, 40_000, false));
        }
    }

    #[test]
    fn ignores_dispute_against_another_clients_deposit() {
        let mut ledger = Ledger::default();
        ledger.apply("deposit,1,10,4\ndeposit,2,20,7\ndispute,2,10");
        ledger.assert_balance(1, (40_000, 0, 40_000, false));
        ledger.assert_balance(2, (70_000, 0, 70_000, false));
    }

    #[test]
    fn dispute_can_make_available_balance_negative() {
        let mut ledger = Ledger::default();
        ledger.apply("deposit,1,10,5.75\nwithdrawal,1,20,4.25");
        ledger.assert_balance(1, (15_000, 0, 15_000, false));
        ledger.apply("dispute,1,10");
        ledger.assert_balance(1, (-42_500, 57_500, 15_000, false));
    }
}
