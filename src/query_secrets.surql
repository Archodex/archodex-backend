LET $selected_resource_ids: array<{id: record<resource>}> = SELECT id FROM resource WHERE resource_type INSIDE ['Secret', 'Secret Value'];

LET $other_resources_to_selected_resource_ids: array<{resources: array<record<resource>>}> = SELECT <-event<-resource.id AS resources FROM (SELECT id FROM $selected_resource_ids);
LET $other_resources_from_selected_resource_ids: array<{resources: array<record<resource>>}> = SELECT ->event->resource.id AS resources FROM (SELECT id FROM $selected_resource_ids);

LET $resource_ids: set<record<resource>> = array::concat(
    $selected_resource_ids.id,
    array::flatten($other_resources_to_selected_resource_ids.resources),
    array::flatten($other_resources_from_selected_resource_ids.resources)
).distinct();

// Add all ancestors of resources. This is tricky because SurrealQL does not
// allow closures to access variables defined in parent scopes. We use
// array::repeat and the entry index in array::map to work around scoping.
$resource_ids = $resource_ids.fold($resource_ids, |$resource_ids: set<record<resource>>, $resource: record<resource>| {
    LET $record_id: array<array<string, 2>> = record::id($resource);
    LET $record_ids: array<array<array<string, 2>>> = array::repeat($record_id, array::len($record_id) - 1);

    RETURN $resource_ids.concat(
        $record_ids.map(|$record_id, $index| type::thing("resource", array::slice($record_id, 0, $index + 1)))
    );
});

$resources = SELECT * FROM resource WHERE id INSIDE $resource_ids;

$events = array::concat(
    array::flatten(SELECT VALUE <-event.* FROM $selected_resource_ids),
    array::flatten(SELECT VALUE ->event.* FROM $selected_resource_ids)
).distinct();