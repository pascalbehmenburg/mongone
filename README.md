# Mongone - MongoDB to ClickHouse Replication

This application synchronizes data from MongoDB to ClickHouse in real-time. It listens to MongoDB change streams and replicates the changes to a ClickHouse database.

## Features

- Real-time replication of MongoDB changes to ClickHouse
- Support for schema evolution (automatically adds new columns)
- Configurable field inclusion/exclusion
- Field name mapping between MongoDB and ClickHouse
- Proper handling of BSON data types

## Prerequisites

- MongoDB
- ClickHouse
- Rust compiler

## Usage

### Basic Usage

```bash
cargo run
```

This will start the application with the default configuration.

## Testing

The application includes a test script that can be used to verify the replication:

```bash
bash test_mongodb_to_clickhouse.sh
```

## Docker

The application can be run with Docker Compose:

```bash
docker-compose up -d
```

This will start both MongoDB and ClickHouse in Docker containers.

## License

MIT 