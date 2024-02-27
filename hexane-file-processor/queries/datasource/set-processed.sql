UPDATE datasource.file SET processed = now()
WHERE id = $1
  AND deleted IS NULL
  AND processed IS NULL
RETURNING id;
