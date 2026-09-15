#[macro_export]
macro_rules! sql_exec {
    ($db:expr, $sql:expr $(, $bind:expr)* $(,)?) => {{
        let sql = $sql;
        match $db.pool() {
            $crate::db_monitor::Pool::Sqlite(p) => sqlx::query(&sql)$(.bind($bind))*
                .execute(p).await.map(|r| r.rows_affected()),
            $crate::db_monitor::Pool::Postgres(p) => sqlx::query(&sql)$(.bind($bind))*
                .execute(p).await.map(|r| r.rows_affected()),
        }
    }};
}

#[macro_export]
macro_rules! sql_fetch_optional {
    ($db:expr, $ty:ty, $sql:expr $(, $bind:expr)* $(,)?) => {{
        let sql = $sql;
        match $db.pool() {
            $crate::db_monitor::Pool::Sqlite(p) => sqlx::query_as::<_, $ty>(&sql)$(.bind($bind))*
                .fetch_optional(p).await,
            $crate::db_monitor::Pool::Postgres(p) => sqlx::query_as::<_, $ty>(&sql)$(.bind($bind))*
                .fetch_optional(p).await,
        }
    }};
}

#[macro_export]
macro_rules! sql_fetch_all {
    ($db:expr, $ty:ty, $sql:expr $(, $bind:expr)* $(,)?) => {{
        let sql = $sql;
        match $db.pool() {
            $crate::db_monitor::Pool::Sqlite(p) => sqlx::query_as::<_, $ty>(&sql)$(.bind($bind))*
                .fetch_all(p).await,
            $crate::db_monitor::Pool::Postgres(p) => sqlx::query_as::<_, $ty>(&sql)$(.bind($bind))*
                .fetch_all(p).await,
        }
    }};
}
