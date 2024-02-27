SELECT id, path, type, user_id
FROM datasource.file
WHERE deleted IS NULL
  AND processed IS NULL
ORDER BY created
LIMIT 1
FOR UPDATE SKIP LOCKED;
