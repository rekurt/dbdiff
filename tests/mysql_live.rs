#![cfg(feature = "mysql")]

use std::time::Duration;

#[tokio::test]
#[ignore = "requires DBDIFF_MYSQL_TEST_DSN and a running MySQL database"]
async fn loading_schema_returns_connection_before_pool_shutdown() {
    let dsn = std::env::var("DBDIFF_MYSQL_TEST_DSN").expect("DBDIFF_MYSQL_TEST_DSN must be set");
    for _ in 0..2 {
        tokio::time::timeout(Duration::from_secs(5), dbdiff::loader::mysql::load(&dsn))
            .await
            .expect("schema loading must not wait forever for its own connection")
            .expect("the live MySQL schema must load successfully");
    }
}
