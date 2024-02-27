SELECT id, username, password, email, verified
FROM users.account
WHERE username = $1
   OR email = $1;
