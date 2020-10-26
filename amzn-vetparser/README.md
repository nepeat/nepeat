# amzn-vetparser
Quick and dirty script to parse Amazon fulfillment / sort center opportunities on A to Z and print them out on the command line.

Why would I do this instead of using the normal VET page to look at it?
* Bug: The page sorts days alphabetically instead of by actual date resulting in hilarious sorting such as the days 1, 11, 12, 2, 3.
* Unavailable VETs are hidden from sight instead of being greyed out or hidden behind a dropdown, making it impossible to see what else has been posted.
* Who doesn't want to see VETs and VTOs outside their own shift or department!

## Requirements
* Python 3
* PyPi packages: click, python-dateutil

## Usage
1. Login to A to Z and open Inspect Element.
2. Open the Network tab in Inspect Element.
3. Browse to the VET or VTO page and look for a `get_opportunities` web request in the Network tab.
4. Copy the JSON result into a file.
5. `python parser.py data.json`
