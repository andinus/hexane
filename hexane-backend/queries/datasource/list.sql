SELECT name, category, to_char(processed, 'YYYY-MM-DD HH24:MI TZ') AS processed, size, hash
FROM datasource.file
WHERE user_id = $1
  AND deleted IS NULL
ORDER BY category, processed;
