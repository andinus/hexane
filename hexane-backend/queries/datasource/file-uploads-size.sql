SELECT SUM(size)::bigint
FROM datasource.file
WHERE user_id = $1
  AND deleted IS NULL;
