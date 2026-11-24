# ultar-auth – English
auth.cli.usage = Usage: ultar-authd --dir DIR passwd USER [--display NAME] [--pin] – the secret is read twice from standard input
auth.cli.done = The secret was stored.
auth.cli.mismatch = The two entries differ.
auth.cli.rejected = Refused: a password has at least 4 characters, a PIN 4 to 12 digits, a user name lower-case Latin letters.
auth.cli.failed = The accounts could not be stored: {detail}
auth.daemon.unusable = The account file is damaged or unreadable; nobody can sign in until an administrator repairs it (ultar-authd passwd).
