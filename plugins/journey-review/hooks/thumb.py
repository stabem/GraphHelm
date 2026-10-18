"""thumb.py <png> [maxW maxH]: print '<w> <h>' then the base64 JPEG of the PNG shrunk to fit maxW x maxH."""
import base64, io, sys
from PIL import Image

LIMIT = 90000  # base64 chars; leaves room for the svg wrapper under 131072

def main(path, max_w, max_h, q0, limit):
    im = Image.open(path).convert("RGB")
    im.thumbnail((max_w, max_h))
    for q in [q for q in (80, 70, 60, 50, 40, 30) if q <= q0]:
        buf = io.BytesIO()
        im.save(buf, "JPEG", quality=q)
        b64 = base64.b64encode(buf.getvalue()).decode()
        if len(b64) <= limit:
            break
    print(im.width, im.height)
    print(b64)

args = sys.argv[1:]
main(args[0], int(args[1]) if len(args) > 1 else 520, int(args[2]) if len(args) > 2 else 325,
     int(args[3]) if len(args) > 3 else 70, int(args[4]) if len(args) > 4 else LIMIT)
