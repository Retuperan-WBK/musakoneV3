import db/migrations
import filepath
import gleam/result
import gleam/string
import logging
import simplifile
import sqlight

pub fn initialize(db_path: String) -> Result(sqlight.Connection, String) {
  let assert Ok(_) = ensure_parent_dir_exists(db_path)
  use db <- result.try(
    sqlight.open(db_path)
    |> result.map_error(fn(e) {
      logging.log(logging.Error, string.inspect(e))
      "Failed to open database: " <> e.message
    }),
  )

  // Enable WAL mode for safe concurrent writes (tracker + state actor)
  use _ <- result.try(
    sqlight.exec("PRAGMA journal_mode=WAL;", db)
    |> result.map_error(fn(e) { "Failed to enable WAL mode: " <> e.message }),
  )

  // Run migrations
  use _ <- result.try(migrations.migrate(db, "src/db/migrations"))

  Ok(db)
}

fn ensure_parent_dir_exists(path: String) -> Result(Nil, String) {
  // Ensure parent directory exists (SQLite will create the file)
  let parent_dir = filepath.directory_name(path)
  use _ <- result.try(case simplifile.is_directory(parent_dir) {
    Ok(True) -> Ok(Nil)
    _ -> {
      case simplifile.create_directory_all(parent_dir) {
        Ok(_) -> {
          logging.log(logging.Notice, "Created directory: " <> parent_dir)
          Ok(Nil)
        }
        Error(e) -> {
          Error(
            "Could not create directory '"
            <> parent_dir
            <> "': "
            <> string.inspect(e),
          )
        }
      }
    }
  })
  Ok(Nil)
}
