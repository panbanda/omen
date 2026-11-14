from flask import Flask
from flask_login import login_required

app = Flask(__name__)


@app.route("/users/<int:user_id>", methods=["POST"])
@login_required
def update(user_id):
    return user_id


if __name__ == "__main__":
    app.run()
