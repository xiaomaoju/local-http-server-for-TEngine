-- Keep historical duplicate names readable by UUID; typed name lookup fails closed.
CREATE INDEX projects_type_download_name ON projects(
    COALESCE(json_extract(config, '$.project_type'), 'bundle'),
    json_extract(config, '$.project_name')
);
