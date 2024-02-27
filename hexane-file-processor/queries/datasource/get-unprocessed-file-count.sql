SELECT COUNT(id)
FROM datasource.file
WHERE deleted IS NULL
  AND processed IS NULL;
