import hashlib
from typing import List

def md5(password: str):
    return hashlib.md5(password.encode("utf8")).hexdigest()

def sha256(password: str):
    return hashlib.sha256(password.encode("utf8")).hexdigest()

def generate_failsafe_password(passwords: List[str]):
    result = ""

    passwords = sorted(passwords)
    for password in passwords:
        result += md5(password)

    return result

if __name__ == "__main__":
    passwords = []

    with open("passwords", "r") as f:
        data = f.read()
        for line in data.split("\n"):
            passwords.append(line.strip())

    combined_password = sha256(generate_failsafe_password(passwords))
    print(f"Veracrypt password\n{combined_password}")
