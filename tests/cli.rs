use std::fs;
use std::path::Path;
use std::process::{self, Command};

#[test]
fn trims_csv_whitespace_and_preserves_four_decimal_places() {
    let input =
        Path::new(env!("CARGO_TARGET_TMPDIR")).join(format!("transactions-{}.csv", process::id()));
    fs::write(
        &input,
        concat!(
            " type , client , tx , amount \n",
            " deposit , 1 , 10 , 0.1001 \n",
            " deposit , 1 , 3 , 0.2002 \n",
            " withdrawal , 1 , 6 , 0.0001 \n",
            " dispute , 1 , 3 , \t\n",
        ),
    )
    .expect("write test CSV");

    let output = Command::new(env!("CARGO_BIN_EXE_payments-engine"))
        .arg(&input)
        .output();
    fs::remove_file(&input).expect("remove test CSV");
    let output = output.expect("run payment engine");
    assert!(output.status.success(), "{output:?}");
    assert!(output.stderr.is_empty(), "{output:?}");
    assert_eq!(
        String::from_utf8(output.stdout).expect("CSV output is UTF-8"),
        "client,available,held,total,locked\n1,0.1000,0.2002,0.3002,false\n",
    );
}
