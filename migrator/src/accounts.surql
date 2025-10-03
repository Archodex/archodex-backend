BEGIN;

DEFINE TABLE IF NOT EXISTS account SCHEMAFULL TYPE NORMAL;
DEFINE FIELD IF NOT EXISTS id ON TABLE account TYPE string READONLY
  // The account ID is a 10-digit number that starts with a non-zero value.
  ASSERT string::len(record::id($this.id)) == 10 && string::is::numeric(record::id($this.id)) && (<number> record::id($this.id) >= 1000000000);
DEFINE FIELD OVERWRITE endpoint ON TABLE account TYPE option<string> READONLY
  ASSERT type::is::none($this.endpoint) OR string::is::url($this.endpoint);
DEFINE FIELD OVERWRITE service_data_surrealdb_url ON TABLE account TYPE option<string> READONLY;
DEFINE FIELD IF NOT EXISTS salt ON TABLE account TYPE bytes READONLY
  ASSERT bytes::len($this.salt) == 16;
DEFINE FIELD IF NOT EXISTS created_at ON TABLE account TYPE datetime READONLY DEFAULT time::now();
DEFINE FIELD IF NOT EXISTS created_by ON TABLE account TYPE record<user> READONLY;
DEFINE FIELD IF NOT EXISTS deleted_at ON TABLE account TYPE option<datetime>;
DEFINE FIELD IF NOT EXISTS deleted_by ON TABLE account TYPE option<record<user>>;

DEFINE TABLE IF NOT EXISTS user SCHEMAFULL TYPE NORMAL;
DEFINE FIELD IF NOT EXISTS id ON TABLE user TYPE uuid READONLY;
DEFINE FIELD IF NOT EXISTS created_at ON TABLE user TYPE datetime READONLY DEFAULT time::now();

DEFINE TABLE IF NOT EXISTS has_access SCHEMAFULL TYPE RELATION FROM user TO account ENFORCED;
DEFINE INDEX IF NOT EXISTS unique ON TABLE has_access FIELDS in, out UNIQUE;
DEFINE FIELD IF NOT EXISTS created_at ON TABLE has_access TYPE datetime READONLY DEFAULT time::now();

COMMIT;