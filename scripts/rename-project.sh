#!/usr/bin/env bash
# Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
# SPDX-License-Identifier: Apache-2.0 OR MIT
#
# This script was used to rename our project from RMC to Kani.

set -o errexit
set -o pipefail
set -o nounset

# Rename all files from old_name to new_name.
rename_files() {
    local OLD_NAME="rmc"
    local NEW_NAME="kani"
    for f in $(git ls-files)
    do
        old=${f}
        new=${old//$OLD_NAME/$NEW_NAME}
        if [ "${old}" != "${new}" ]
        then
            # Ensure target folder exists.
            echo "${old} -> ${new}"
            mkdir -p $(dirname "${new}")
            git mv "${old}" "${new}"
        fi
    done
}

# Find all symbolic links and recreate the ones that pointed at old file.
rename_links() {
    local OLD_NAME="rmc"
    local NEW_NAME="kani"
    for f in $(find . -type l)
    do
        pushd $(dirname ${f}) > /dev/null
        slink=$(basename "${f}")
        target=$(readlink "${slink}")
        if [ "${target/rmc/}" != ${target} ]
        then
            old=${target}
            new=${old//$OLD_NAME/$NEW_NAME}
            echo "${f} -> (${old} -> ${new})"

            rm "${slink}"
            ln -s "${new}" "${slink}"
        fi
        popd > /dev/null
    done
}

# Replace all variations of old_name by respective new_names.
replace_name() {
    echo "--- Replace names"
    local OLD_NAMES=("rmc" "an RMC" "RMC_" "RMC" "Rmc")
    local NEW_NAMES=("kani" "a Kani" "KANI_" "Kani" "Kani")
    last=$(expr ${#OLD_NAMES[@]} - 1)
    for n in $(seq 0 ${last})
    do
        echo "${n}: ${OLD_NAMES[$n]} -> ${NEW_NAMES[$n]}"
    done
    echo "Note: This script will not rename github links"

    for f in $(git ls-files)
    do
        if [[ $(grep --color=auto -i rmc "${f}") ]]
        then
            # Display occurrences
            echo -e "\e[32m${f}\e[0m"
            grep --color=auto -i rmc "${f}"

            echo -n -e "\e[32m"
            read -p "Should rename? [n/y(default)]:" confirm
            if [ "${confirm:-y}" == "y" ]
            then
                for n in $(seq 0 ${last})
                do
                    sed -i "s/${OLD_NAMES[$n]}/${NEW_NAMES[$n]}/g" ${f}
                    # This may incorrectly replace github links. Fix this.
                    glink="github.com\/model-checking\/rmc"
                    wrong_link="github.com\/model-checking\/kani"
                    sed -i "s/${wrong_link}/${glink}/g" ${f}

                    glink="github.com:model-checking\/rmc"
                    wrong_link="github.com:model-checking\/kani"
                    sed -i "s/${wrong_link}/${glink}/g" ${f}
                done
            else
                echo "${f}" >> /tmp/rejected.txt
            fi
            echo -e "\e[0m"
        fi
    done
}

# Uncomment the step you would like to perform. Make sure you audit the results
# before moving to the next step.
#
# rename_files
# rename_links
# replace_name
# ./scripts/kani-regression.sh
