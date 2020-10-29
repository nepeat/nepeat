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

class OpportunitiesItem:
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

    @property
    def vto(self):
        return self.data["opportunity_type"] == "vto"

    @property
    def vet(self):
        return self.data["opportunity_type"] == "vet"

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
def parse_file(vetfile):
    opportunities = []

    # VET files are whole responses from the /api/v1/opportunities/get_opportunities endpoint.
    # Grab these with the Inspect Element network tab.
    # There probably is a way to interact with AMZN IDP but that's too much effort for this quick hackjob.
    data = json.load(vetfile)

    # Quick sanity check for empty files and other weird JSON files.
    if "vetOpportunities" not in data:
        print("Not a valid opportunities file.")
        return

    # Add all the VETs and VTOs to an array and sort it by date as actual dates instead of strings. ;)
    for vet in data["vetOpportunities"]:
        opportunities.append(OpportunitiesItem(vet))

    for vto in data["vtoOpportunities"]:
        opportunities.append(OpportunitiesItem(vto))

    opportunities.sort(key=lambda _: _.start_time)

    # Disgusting code to print it all out on the console.
    for opportunitiy in opportunities:
        color = ""

        # Green for taken.
        # Red for unavailable.

        if opportunitiy.inactive_reason == "ALREADY_ACCEPTED":
            color = ANSIColor.OKBLUE
        elif not opportunitiy.inactive_reason:
            color = ANSIColor.OKGREEN
        else:
            color = ANSIColor.FAIL

        print(" ".join([
            color,
            f"[{opportunitiy.opportunity_type}] [{opportunitiy.workgroup}]",
            opportunitiy.start_time.astimezone().strftime("%D %H:%M"),
            "-",
            opportunitiy.end_time.astimezone().strftime("%H:%M"),
            opportunitiy.incentives,
            opportunitiy.inactive_reason or "AVAILABLE",
            ANSIColor.ENDC
        ]).strip())

if __name__ == "__main__":
    parse_file()
