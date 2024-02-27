SELECT DISTINCT(category)
FROM datasource.file
WHERE user_id = $1
  AND deleted IS NULL;
