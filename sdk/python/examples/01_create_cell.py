"""01_create_cell:创建 Cell(懒创建,零 IO)。"""
from helpers import client

combee = client()
cell = combee.cells.create(name="my-app")
print("cell id:", cell.id)
