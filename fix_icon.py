from PIL import Image
try:
    img = Image.open('f:/NexTune-Tauri/assets/icon.png')
    img.save('f:/NexTune-Tauri/assets/icon_fixed.png', 'PNG')
    print("Success")
except Exception as e:
    print(f"Error: {e}")
