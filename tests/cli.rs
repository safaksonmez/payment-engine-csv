use std::fs;
use std::path::Path;
use std::process::{self, Command, Output};

fn run_csv(name: &str, csv: &str) -> Output {
    let input =
        Path::new(env!("CARGO_TARGET_TMPDIR")).join(format!("{name}-{}.csv", process::id()));
    fs::write(&input, csv).expect("write test CSV");

    let output = Command::new(env!("CARGO_BIN_EXE_payments-engine"))
        .arg(&input)
        .output();
    fs::remove_file(&input).expect("remove test CSV");
    output.expect("run payment engine")
}

#[test]
fn trims_csv_whitespace_and_preserves_four_decimal_places() {
    let output = run_csv(
        "whitespace",
        concat!(
            " type , client , tx , amount \n",
            " deposit , 1 , 10 , 0.1001 \n",
            " deposit , 1 , 3 , 0.2002 \n",
            " withdrawal , 1 , 6 , 0.0001 \n",
            " dispute , 1 , 3 , \t\n",
        ),
    );

    assert!(output.status.success(), "{output:?}");
    assert!(output.stderr.is_empty(), "{output:?}");
    assert_eq!(
        String::from_utf8(output.stdout).expect("CSV output is UTF-8"),
        "client,available,held,total,locked\n1,0.1000,0.2002,0.3002,false\n",
    );
}

#[test]
fn rejects_balance_overflow_without_writing_csv() {
    let output = run_csv(
        "overflow",
        concat!(
            "type,client,tx,amount\n",
            "deposit,1,1,922337203685477.5807\n",
            "dispute,1,1,\n",
            "deposit,1,2,0.0001\n",
        ),
    );

    assert!(
        output.status.code().is_some_and(|code| code != 0),
        "{output:?}"
    );
    assert!(output.stdout.is_empty(), "{output:?}");
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("CSV line 4: balance exceeds the i64 fixed-point range"),
        "{stderr}",
    );
}

#[test]
fn chargeback_can_leave_a_locked_account_with_a_negative_total() {
    let output = run_csv(
        "negative-chargeback",
        concat!(
            "type,client,tx,amount\n",
            "deposit,1,1,5.7500\n",
            "withdrawal,1,2,4.2500\n",
            "dispute,1,1,\n",
            "chargeback,1,1,\n",
        ),
    );

    assert!(output.status.success(), "{output:?}");
    assert!(output.stderr.is_empty(), "{output:?}");
    assert_eq!(
        String::from_utf8(output.stdout).expect("CSV output is UTF-8"),
        "client,available,held,total,locked\n1,-4.2500,0.0000,-4.2500,true\n",
    );
}
