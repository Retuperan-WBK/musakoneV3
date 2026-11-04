import gleam/dynamic/decode
import gleam/int
import gleam/list
import gleam/result
import gleam/string
import logging
import simplifile
import sqlight

/// Migration represents a single SQL migration file
pub type Migration {
  Migration(version: Int, name: String, sql: String)
}

/// Run all pending migrations on the database
pub fn migrate(
  db: sqlight.Connection,
  migrations_dir: String,
) -> Result(Nil, String) {
  // Get current version
  use current_version <- result.try(get_current_version(db))

  // Load all migrations from directory
  use migrations <- result.try(load_migrations(migrations_dir))

  // Filter pending migrations
  let pending =
    list.filter(migrations, fn(m) { m.version > current_version })
    |> list.sort(by: fn(a, b) { int.compare(a.version, b.version) })

  case pending {
    [] -> {
      logging.log(logging.Info, "No pending migrations")
      Ok(Nil)
    }
    _ -> {
      logging.log(
        logging.Info,
        "Running " <> int.to_string(list.length(pending)) <> " migration(s)...",
      )
      apply_migrations(db, pending)
    }
  }
}

/// Get the current database version from PRAGMA user_version
fn get_current_version(db: sqlight.Connection) -> Result(Int, String) {
  let decoder = decode.at([0], decode.int)

  sqlight.query("PRAGMA user_version;", db, [], decoder)
  |> result.map_error(fn(_) { "Failed to get database version" })
  |> result.try(fn(rows) {
    case rows {
      [version] -> Ok(version)
      _ -> Ok(0)
    }
  })
}

/// Set the database version using PRAGMA user_version
fn set_version(db: sqlight.Connection, version: Int) -> Result(Nil, String) {
  let sql = "PRAGMA user_version = " <> int.to_string(version) <> ";"

  sqlight.query(sql, db, [], decode.dynamic)
  |> result.map_error(fn(_) {
    "Failed to set database version to " <> int.to_string(version)
  })
  |> result.map(fn(_) { Nil })
}

/// Load all migration files from the migrations directory
fn load_migrations(migrations_dir: String) -> Result(List(Migration), String) {
  simplifile.read_directory(migrations_dir)
  |> result.map_error(fn(e) {
    "Failed to read migrations directory: "
    <> migrations_dir
    <> " - "
    <> string.inspect(e)
  })
  |> result.try(fn(files) {
    files
    |> list.filter(fn(file) { string.ends_with(file, ".sql") })
    |> list.try_map(fn(file) {
      parse_migration_filename(file)
      |> result.try(fn(parsed) {
        let #(version, name) = parsed
        let path = migrations_dir <> "/" <> file
        simplifile.read(path)
        |> result.map_error(fn(_) { "Failed to read migration file: " <> path })
        |> result.map(fn(sql) {
          Migration(version: version, name: name, sql: sql)
        })
      })
    })
  })
}

/// Parse migration filename in format V1__name.sql
fn parse_migration_filename(
  filename: String,
) -> Result(#(Int, String), String) {
  case string.split(filename, "__") {
    [version_part, name_part] -> {
      // Extract version number from V1 format
      let version_str = string.drop_start(version_part, 1)

      int.parse(version_str)
      |> result.map_error(fn(_) { "Invalid version number in: " <> filename })
      |> result.map(fn(version) {
        let name = string.replace(name_part, ".sql", "")
        #(version, name)
      })
    }
    _ -> Error("Invalid migration filename format: " <> filename)
  }
}

/// Apply a list of migrations in order
fn apply_migrations(
  db: sqlight.Connection,
  migrations: List(Migration),
) -> Result(Nil, String) {
  list.try_each(migrations, fn(migration) {
    logging.log(
      logging.Info,
      "Applying migration V"
        <> int.to_string(migration.version)
        <> "__"
        <> migration.name,
    )

    // Execute the migration SQL
    sqlight.exec(migration.sql, db)
    |> result.map_error(fn(e) {
      "Migration V"
      <> int.to_string(migration.version)
      <> " failed: "
      <> e.message
    })
    |> result.try(fn(_) {
      // Update version after successful migration
      set_version(db, migration.version)
    })
  })
}
