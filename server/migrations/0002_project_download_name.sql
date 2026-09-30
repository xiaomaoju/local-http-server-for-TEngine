-- Non-unique so pre-existing duplicate names never prevent an upgrade.
-- New creates/renames reject collisions; ambiguous public names fail closed.
CREATE INDEX projects_download_name ON projects(json_extract(config, '$.project_name'));
