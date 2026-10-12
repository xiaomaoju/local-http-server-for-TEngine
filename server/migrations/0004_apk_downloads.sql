-- Stable identities survive project-version renames and isolate same-name recreation.
CREATE TABLE apk_download_scopes (
    id TEXT PRIMARY KEY,
    project_id TEXT NOT NULL REFERENCES projects(id) ON DELETE CASCADE,
    project_version TEXT NOT NULL,
    UNIQUE(project_id, project_version)
);
CREATE TABLE apk_download_counts (
    scope_id TEXT NOT NULL REFERENCES apk_download_scopes(id) ON DELETE CASCADE,
    release TEXT NOT NULL,
    downloads INTEGER NOT NULL DEFAULT 0 CHECK(downloads >= 0),
    PRIMARY KEY(scope_id, release)
);

INSERT INTO apk_download_scopes(id, project_id, project_version)
SELECT lower(hex(randomblob(16))), p.id, json_extract(v.value, '$.name')
FROM projects p, json_each(p.config, '$.project_versions') v
WHERE json_extract(p.config, '$.project_type') = 'apk';

CREATE TRIGGER apk_download_scopes_insert AFTER INSERT ON projects
WHEN json_extract(NEW.config, '$.project_type') = 'apk'
BEGIN
    INSERT INTO apk_download_scopes(id, project_id, project_version)
    SELECT lower(hex(randomblob(16))), NEW.id, json_extract(value, '$.name')
    FROM json_each(NEW.config, '$.project_versions');
END;

CREATE TRIGGER apk_download_scopes_update AFTER UPDATE OF config ON projects
WHEN json_extract(NEW.config, '$.project_type') = 'apk'
BEGIN
    DELETE FROM apk_download_scopes WHERE project_id = NEW.id
    AND project_version NOT IN (SELECT json_extract(value, '$.name') FROM json_each(NEW.config, '$.project_versions'));
    INSERT INTO apk_download_scopes(id, project_id, project_version)
    SELECT lower(hex(randomblob(16))), NEW.id, json_extract(value, '$.name')
    FROM json_each(NEW.config, '$.project_versions')
    WHERE true
    ON CONFLICT(project_id, project_version) DO NOTHING;
END;
