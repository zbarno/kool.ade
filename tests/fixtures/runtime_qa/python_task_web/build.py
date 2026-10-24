from pathlib import Path
import shutil


root = Path(__file__).parent
output = root / "dist"
output.mkdir(exist_ok=True)
shutil.copyfile(root / "src" / "server.py", output / "app.py")
shutil.copyfile(root / "src" / "index.html", output / "index.html")
