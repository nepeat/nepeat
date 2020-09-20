import json

import click
import dateutil.parser

# https://stackoverflow.com/questions/287871/how-to-print-colored-text-in-python
class ANSIColor:
    HEADER = '\033[95m'
    OKBLUE = '\033[94m'
    OKGREEN = '\033[92m'
    WARNING = '\033[93m'
    FAIL = '\033[91m'
    ENDC = '\033[0m'
    BOLD = '\033[1m'
    UNDERLINE = '\033[4m'

class VETItem:
    def __init__(self, data: dict):
        self.data = data

    def __getattr__(self, attr):
        if attr in self.data:
            return self.data[attr]

    @property
    def inactive_reason(self):
        return self.data.get("inactive_reason", None)

    @property
    def posted_time(self):
        return dateutil.parser.parse(self.data["signup_start_time"])
    
    @property
    def start_time(self):
        return dateutil.parser.parse(self.data["start_time"])

    @property
    def end_time(self):
        return dateutil.parser.parse(self.data["end_time"])

    @property
    def length(self):
        return self.data["minutes_to_cover_opportunity"]

    """
        Parses surge pay ncentives and turns it into a human readable string.
    """
    @property
    def incentives(self):
        incentives = self.data["incentives"]
        if not incentives:
            return ""

        # Breaks if there are multiple incentives but there is likely to only be one?
        incentive = incentives[0]
        if incentive["incentive_type"] == "AdditiveDynamicCompensation":
            return f"+${incentive['incentive_value']}/hr"

        # I believe there's only AdditiveDynamicCompensation?
        # Raise an exception if not so that I can fix it later.
        raise ValueError(f"Unknown incentive type {incentive['incentive_type']}")

@click.command()
@click.argument("vetfile", type=click.File("r"))
def parse_vet(vetfile):
    vets = []

    # VET files are whole responses from the /api/v1/opportunities/get_opportunities endpoint.
    # Grab these with the Inspect Element network tab.
    # There probably is a way to interact with AMZN IDP but that's too much effort for this quick hackjob.
    data = json.load(vetfile)

    # Quick sanity check for empty files and other weird JSON files.
    if "vetOpportunities" not in data:
        print("Not a valid opportunities file.")
        return

    # Add all the VETs to an array and sort it by date as actual dates instead of strings. ;)
    for vet in data["vetOpportunities"]:
        vets.append(VETItem(vet))
    
    vets.sort(key=lambda _: _.start_time)

    # Disgusting code to print it all out on the console.
    for vet in vets:
        color = ""

        # Green for taken.
        # Red for unavailable.

        if not vet.inactive_reason or "ALREADY_ACCEPTED" in vet.inactive_reason:
            color = ANSIColor.OKGREEN
        else:
            color = ANSIColor.FAIL

        print(" ".join([
            color,
            vet.start_time.astimezone().strftime("%D %H:%M"),
            "-",
            vet.end_time.astimezone().strftime("%H:%M"),
            vet.incentives,
            vet.inactive_reason or "AVAILABLE",
            ANSIColor.ENDC
        ]))

if __name__ == "__main__":
    parse_vet()