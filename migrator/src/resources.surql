BEGIN;

DEFINE TABLE IF NOT EXISTS report_api_key SCHEMAFULL TYPE NORMAL;
DEFINE FIELD IF NOT EXISTS id ON TABLE report_api_key TYPE int READONLY
    ASSERT $this.id >= 0;
DEFINE FIELD IF NOT EXISTS description ON TABLE report_api_key TYPE option<string>;
DEFINE FIELD IF NOT EXISTS version ON TABLE report_api_key TYPE int READONLY VALUE 1;
DEFINE FIELD IF NOT EXISTS created_at ON TABLE report_api_key TYPE datetime READONLY DEFAULT time::now();
DEFINE FIELD IF NOT EXISTS created_by ON TABLE report_api_key TYPE record<user> READONLY;
DEFINE FIELD IF NOT EXISTS revoked_at ON TABLE report_api_key TYPE option<datetime>;
DEFINE FIELD IF NOT EXISTS revoked_by ON TABLE report_api_key TYPE option<record<user>>;

DEFINE TABLE IF NOT EXISTS resource SCHEMAFULL TYPE NORMAL;
DEFINE FIELD IF NOT EXISTS id ON TABLE resource TYPE array<array<string, 2>> READONLY;
DEFINE FIELD IF NOT EXISTS resource_type ON TABLE resource TYPE string READONLY DEFAULT array::last(record::id($this.id))[0];
DEFINE INDEX IF NOT EXISTS resource_type ON TABLE resource FIELDS resource_type;
DEFINE FIELD IF NOT EXISTS resource_id ON TABLE resource TYPE string READONLY DEFAULT array::last(record::id($this.id))[1];
DEFINE FIELD IF NOT EXISTS environments ON TABLE resource TYPE set<string> DEFAULT ALWAYS [];
DEFINE FIELD IF NOT EXISTS first_seen_at ON TABLE resource TYPE datetime READONLY;
DEFINE FIELD IF NOT EXISTS last_seen_at ON TABLE resource TYPE datetime;
DEFINE FIELD IF NOT EXISTS attributes ON TABLE resource FLEXIBLE TYPE object DEFAULT {};

// ON DUPLICATE KEY UPDATE doesn't change anything, but prevents erroring if the
// record already exists
INSERT INTO resource {
  id: [],
  resource_type: "Archodex Root",
  resource_id: "Root",
  first_seen_at: time::now(),
  last_seen_at: time::now()
} ON DUPLICATE KEY UPDATE resource_id = "Root" RETURN NONE;

DEFINE TABLE IF NOT EXISTS contains SCHEMAFULL TYPE RELATION FROM resource TO resource ENFORCED;
DEFINE INDEX IF NOT EXISTS unique ON contains FIELDS out UNIQUE;
DEFINE FIELD IF NOT EXISTS first_seen_at ON TABLE contains TYPE datetime READONLY;
DEFINE FIELD IF NOT EXISTS last_seen_at ON TABLE contains TYPE datetime;

DEFINE TABLE IF NOT EXISTS principal_chain SCHEMAFULL TYPE NORMAL;
DEFINE FIELD IF NOT EXISTS id ON TABLE principal_chain FLEXIBLE TYPE array<object> READONLY
    // Assert chain is an array of principals, where a principal has a resource
    // ID (`id`) and optionally an event type (`event`). The event type should
    // be a string for all principals except the last.
    ASSERT $this.id.all(|$principal|
        object::len($principal) <= 2 &&
        (type::is::none($principal.event) || type::is::string($principal.event)) &&
        type::is::array($principal.id) && $principal.id.all(|$id|
            type::is::array($id) && array::len($id) == 2 && $id.all(|$part|
                type::is::string($part)
            )
        )
    );
DEFINE FIELD IF NOT EXISTS first_seen_at ON TABLE principal_chain TYPE datetime READONLY;
DEFINE FIELD IF NOT EXISTS last_seen_at ON TABLE principal_chain TYPE datetime;

DEFINE TABLE IF NOT EXISTS event SCHEMAFULL TYPE RELATION FROM resource TO resource ENFORCED;
DEFINE FIELD IF NOT EXISTS type ON TABLE event TYPE string READONLY;
DEFINE INDEX IF NOT EXISTS unique ON TABLE event FIELDS in, out, type UNIQUE;
DEFINE FIELD IF NOT EXISTS principal_chains ON TABLE event TYPE set<record<principal_chain>>;
DEFINE FIELD IF NOT EXISTS has_direct_principal_chain ON TABLE event TYPE bool;
DEFINE FIELD IF NOT EXISTS first_seen_at ON TABLE event TYPE datetime READONLY;
DEFINE FIELD IF NOT EXISTS last_seen_at ON TABLE event TYPE datetime;

// Fetch all globally unique ancestors of a set of resources. For example, the
// set may contain an S3 Object. This function will notice that the S3 Bucket
// that contains the object is a globally unique resource, but then it will
// check if the Bucket has a container of its own. It will find that it does, a
// region in the commercial partition of AWS. The region resource will be one of
// the resources returned. The function is recursive, in case there are multiple
// globally unique ancestors of a resource.
DEFINE FUNCTION IF NOT EXISTS fn::fetch_global_containers($resources: set<record<resource>>) -> set<object> {
    // Get all globally unique resources in the current set of resources by
    // creating a resource ID from the first element of the passed in resource
    // IDs.
    $resource_roots = $resources.map(|$resource| type::thing("resource", [record::id($resource)[0]])).distinct();

    // Select the container of each globally unique resource, but ignore
    // containers that are the root resource (resource:[])
    $root_containers = (SELECT <-contains<-resource AS id, id AS contains FROM $resource_roots /*PARALLEL*/)
        .map(|$record| { id: $record.id[0], contains: $record.contains })
        .filter(|$root_container| $root_container.id != NONE && record::id($root_container.id) != []);

    RETURN if !array::is_empty($root_containers) {
        array::concat(
            $root_containers,
            fn::fetch_global_containers($root_containers.map(|$root_container| $root_container.id))
        );
    } else {
        $root_containers;
    }
};

COMMIT;