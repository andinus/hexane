SELECT credit,
       (SELECT SUM(size)::bigint
        FROM datasource.file
        WHERE user_id = account.id) AS file_uploaded

FROM users.account
WHERE id = $1
  AND deleted IS NULL;
