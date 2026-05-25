# Icoon

`sovacount.ico` ontbreekt momenteel in deze pack.

**Genereer hem zelf** uit het bestaande macOS `icon.icns`:

```bash
# Op macOS — vereist Pillow (pip install Pillow):
python3 -c "
from PIL import Image
import os
img = Image.open('../../LAUNCHER/SovaCount.app/Contents/Resources/icon.icns')
img.save('sovacount.ico', format='ICO', sizes=[(16,16),(32,32),(48,48),(64,64),(128,128),(256,256)])
"
```

Of, op Linux/Mac met ImageMagick:
```bash
magick ../../LAUNCHER/SovaCount.app/Contents/Resources/icon.icns -resize 256x256 sovacount.ico
```

Op Windows: gebruik een online .icns→.ico converter, of [IcoFX](https://icofx.ro).

Zonder `.ico` werkt SovaCount nog perfect — alleen de desktop-snelkoppeling
krijgt het default Windows-icoon.
