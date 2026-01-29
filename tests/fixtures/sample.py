class UserService:
    def __init__(self, db):
        self.db = db

    def get_user(self, user_id):
        if not user_id:
            raise ValueError("user_id required")
        return self.db.find(user_id)

    def create_user(self, name, email):
        if not name or not email:
            raise ValueError("name and email required")
        if "@" not in email:
            raise ValueError("invalid email")
        return self.db.insert({"name": name, "email": email})


def calculate_discount(price, tier):
    # TODO: refactor this into a strategy pattern
    if tier == "gold":
        return price * 0.20
    elif tier == "silver":
        return price * 0.10
    elif tier == "bronze":
        return price * 0.05
    else:
        return 0
