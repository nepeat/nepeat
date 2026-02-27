import json
from concurrent.futures import ThreadPoolExecutor

import httpx
from bs4 import BeautifulSoup


def get_permits(parcel: str):
    permits = set()

    url = f"https://permitsearch.mybuildingpermit.com/PermitDetails/PermitsForParcelData/1/{parcel}"
    r = httpx.get(url)
    r.raise_for_status()

    for permit in r.json():
        permits.add(permit["PermitNumber"])

    return permits


def get_permit(permit_number: str):
    permit = {}

    # get the permit info
    info = {}
    url = f"https://permitsearch.mybuildingpermit.com/PermitDetails/{permit_number}/Bellevue"
    r = httpx.get(url)
    r.raise_for_status()

    soup = BeautifulSoup(r.text, "html.parser")
    # data tables have .table-striped
    tables = soup.find_all("table", {"class": "table-striped"})
    for table in tables:
        # data is in tr -> th, td
        rows = table.find_all("tr")
        for row in rows:
            th = row.find("th")
            td = row.find("td")
            if th and td:
                info[th.text] = td.text.strip()

    permit["Info"] = info

    # get the permit details
    detail_types = [
        "PermitDocuments",
        "PeopleData",
        "PermitFeeData",
        "ReviewsAndActivity",
        "PermitInspections",
        "PermitConditionsData",
    ]

    for detail_type in detail_types:
        url = f"https://permitsearch.mybuildingpermit.com/PermitDetails/{detail_type}/{permit_number}/1"
        r = httpx.get(url)
        r.raise_for_status()

        permit[detail_type] = r.json()

    return permit


def save_permit(permit_number: str):
    permit = get_permit(permit_number)
    print(f"Got permit {permit_number}")
    with open("permits/" + permit_number + ".json", "w") as f:
        json.dump(permit, f, indent=2, sort_keys=True)


if __name__ == "__main__":
    # key center
    permits = get_permits("1544100219")

    with ThreadPoolExecutor(max_workers=8) as executor:
        executor.map(save_permit, permits)

    # lincoln square north
    permits = get_permits("4323430010")

    with ThreadPoolExecutor(max_workers=8) as executor:
        executor.map(save_permit, permits)
