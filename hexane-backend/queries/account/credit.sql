SELECT credit
FROM users.account
WHERE id = $1
  AND deleted IS NULL;
