import datetime
def str_to_ms(s):
    dt = datetime.datetime.strptime(str(s), "%Y%m%d")
    return int(dt.timestamp() * 1000)

for d in [20220101, 20220601]:
    print(d, str_to_ms(d))
