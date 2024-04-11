extern crate polars;

use polars::prelude::*;

// Store each TransactionMetric in a dataframe.
use crate::metrics::TransactionMetric;

// Global DataFrame to store transaction logs
let mut df: DataFrame = df!(
    "elapsed" =>
);
static TRANSACTION_LOGS: Lazy<Mutex<DataFrame>> = Lazy::new(|| {
    let schema = Schema::new(vec![
        Field::new("elapsed", DataType::UInt64),
        Field::new("scenario_index", DataType::UInt64),
        Field::new("transaction_index", DataType::UInt64),
        Field::new("name", DataType::Utf8),
        Field::new("run_time", DataType::UInt64),
        Field::new("success", DataType::Boolean),
        Field::new("user", DataType::UInt64),
    ]);

    let df = DataFrame::empty(&schema);
    Mutex::new(df)
});

// Function to log a transaction
fn log_transaction(transaction: Transaction) {
    let mut df = TRANSACTION_LOGS.lock().unwrap();

    // Create a Series for each field
    let elapsed_series = Series::new("elapsed", &[transaction.elapsed]);
    let scenario_index_series = Series::new("scenario_index", &[transaction.scenario_index]);
    let transaction_index_series =
        Series::new("transaction_index", &[transaction.transaction_index]);
    let name_series = Series::new("name", &[transaction.name]);
    let run_time_series = Series::new("run_time", &[transaction.response_time]);
    let success_series = Series::new("success", &[transaction.success]);
    let user_series = Series::new("user", &[transaction.user]);

    // Create a new DataFrame for the current transaction
    let transaction_df = DataFrame::new(vec![
        elapsed_series,
        scenario_index_series,
        transaction_index_series,
        name_series,
        run_time_series,
        success_series,
        user_series,
    ])
    .unwrap();

    // Append the current transaction to the global DataFrame
    *df = df.vstack(&transaction_df).unwrap();
}
