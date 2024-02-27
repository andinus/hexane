DELETE FROM datasource.file
WHERE user_id = $1
  AND hash = $2
RETURNING path;
