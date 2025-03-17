use mongodb::{Client as MongoClient, options::ClientOptions};
use mongodb::bson::Bson;
use urlencoding::encode;
use clickhouse::Client as ClickhouseClient;
use std::collections::HashSet;
use futures_util::StreamExt;
use dotenv::dotenv;
use std::env;

fn get_env_var(name: &str) -> String {
    env::var(name).unwrap_or_else(|_| panic!("Environment variable {} not set", name))
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Load environment variables from .env file
    dotenv().ok();
    
    // MongoDB connection
    let username = get_env_var("MONGODB_USER");
    let binding = get_env_var("MONGODB_PASSWORD");
    let password = encode(&binding);
    let host = get_env_var("MONGODB_HOST");
    let port = get_env_var("MONGODB_PORT");
    let admin_db = get_env_var("MONGODB_ADMIN_DB");
    let uri = format!("mongodb://{}:{}@{}:{}/{}", username, password, host, port, admin_db);
    let client_options = ClientOptions::parse(uri).await?;
    let mongo_client = MongoClient::with_options(client_options)?;
    
    // ClickHouse connection
    let clickhouse_host = get_env_var("CLICKHOUSE_HOST");
    let clickhouse_port = get_env_var("CLICKHOUSE_PORT");
    let clickhouse_user = get_env_var("CLICKHOUSE_USER");
    let clickhouse_password = get_env_var("CLICKHOUSE_PASSWORD");
    let clickhouse_db = get_env_var("CLICKHOUSE_DB");
    
    println!("Connecting to ClickHouse at: http://{}:{}/ as user: {}, database: {}", 
             clickhouse_host, clickhouse_port, clickhouse_user, clickhouse_db);
    
    let clickhouse_client = ClickhouseClient::default()
        .with_url(&format!("http://{}:{}", clickhouse_host, clickhouse_port))
        .with_user(clickhouse_user)
        .with_password(clickhouse_password)
        .with_database(clickhouse_db);
    
    let create_table_query = "
        CREATE TABLE IF NOT EXISTS mongodb_changes (
            _id String,
            itemid String,
            operation_type String,
            collection String,
            type String,
            timestamp DateTime DEFAULT now()
        ) ENGINE = MergeTree()
        ORDER BY (timestamp, _id)
    ";
    clickhouse_client.query(create_table_query).execute().await?;
    
    // Track known columns to handle schema evolution
    let mut known_columns: HashSet<String> = HashSet::new();
    known_columns.insert("_id".to_string());  // Always include _id column
    known_columns.insert("itemid".to_string());
    known_columns.insert("operation_type".to_string());
    known_columns.insert("collection".to_string());
    known_columns.insert("type".to_string());
    known_columns.insert("timestamp".to_string());
    
    let mut change_stream = mongo_client.watch().await?;
    
    println!("Watching for MongoDB changes...");
    
    while change_stream.is_alive() {
        match change_stream.next().await {
            Some(Ok(change)) => {
                let id = change.id;
                println!("Received change event - Id: {:?}", id);

                if let Some(doc) = change.full_document {
                    println!("Document: {:?}", doc);
                    
                    // Extract document fields
                    let operation_type = format!("{:?}", change.operation_type);
                    let collection = change.ns.as_ref()
                        .map(|ns| ns.coll.clone())
                        .unwrap_or_else(|| Some("unknown".to_string()));
                    
                    // Check for new columns and add them if needed
                    let doc_columns: HashSet<String> = doc.keys().map(|k| k.to_string()).collect();
                    let new_columns: Vec<String> = doc_columns.difference(&known_columns).cloned().collect();
                    
                    if !new_columns.is_empty() {
                        println!("Adding new columns: {:?}", new_columns);
                        // Add new columns to the table
                        for column in &new_columns {
                            // Escape column name for ClickHouse
                            let safe_column = escape_column_name(column);
                            let query = format!(
                                "ALTER TABLE mongodb_changes ADD COLUMN IF NOT EXISTS {} String",
                                safe_column
                            );
                            clickhouse_client.query(&query).execute().await?;
                            known_columns.insert(column.clone());
                        }
                    }
                    
                    // Insert the document data into ClickHouse
                    // Convert BSON document to a format suitable for ClickHouse
                    let mut values = Vec::new();
                    let mut columns = Vec::new();
                    
                    // Add standard fields
                    columns.push("operation_type".to_string());
                    values.push(format!("'{}'", operation_type));
                    
                    columns.push("collection".to_string());
                    values.push(format!("'{:?}'", collection));
                    
                    // Add document fields
                    for (key, value) in &doc {
                        let safe_column = escape_column_name(key);
                        columns.push(safe_column);
                        
                        // Convert BSON value to string representation
                        let string_value = bson_to_clickhouse_value(value);
                        values.push(string_value);
                    }
                    
                    let query = format!(
                        "INSERT INTO mongodb_changes ({}) VALUES ({})",
                        columns.join(", "),
                        values.join(", ")
                    );
                    
                    match clickhouse_client.query(&query).execute().await {
                        Ok(_) => println!("Data inserted into ClickHouse"),
                        Err(e) => eprintln!("Error inserting data: {:?}", e),
                    }
                }
            },
            Some(Err(e)) => {
                eprintln!("Error processing change stream: {:?}", e);
            },
            None => {
                println!("Change stream ended");
                break;
            }
        }
    }
    
    Ok(())
}

// Function to properly escape column names for ClickHouse
fn escape_column_name(name: &str) -> String {
    // If column name contains spaces or special characters, wrap it in backticks
    if name.contains(' ') || name.contains('-') || name.contains('.') || name.chars().any(|c| !c.is_alphanumeric() && c != '_') {
        format!("`{}`", name.replace("`", "\\`"))
    } else {
        name.to_string()
    }
}

// Function to convert BSON values to ClickHouse string representation
fn bson_to_clickhouse_value(value: &Bson) -> String {
    match value {
        Bson::String(s) => format!("'{}'", s.replace("'", "\\'")),
        Bson::ObjectId(oid) => format!("'{}'", oid.to_string()),
        Bson::Document(doc) => format!("'{}'", serde_json::to_string(doc).unwrap_or_default().replace("'", "\\'")),
        Bson::Array(arr) => format!("'{}'", serde_json::to_string(arr).unwrap_or_default().replace("'", "\\'")),
        Bson::Boolean(b) => format!("{}", if *b { 1 } else { 0 }),
        Bson::Int32(i) => format!("{}", i),
        Bson::Int64(i) => format!("{}", i),
        Bson::Double(d) => format!("{}", d),
        Bson::DateTime(dt) => format!("'{}'", dt.to_string().replace("'", "\\'")),
        Bson::Null => format!("NULL"),
        _ => format!("'{}'", value.to_string().replace("'", "\\'")),
    }
}


