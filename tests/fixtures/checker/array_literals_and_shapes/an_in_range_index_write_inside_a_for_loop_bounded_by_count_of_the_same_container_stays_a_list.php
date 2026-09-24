<?php
            /**
             * @param list<int|string> $chronologyDate
             */
            function render(array $chronologyDate): void {
                $page_chronology_date = $chronologyDate;
                while (count($page_chronology_date) > 3) {
                    array_pop($page_chronology_date);
                }
                for ($i = 0; $i < count($page_chronology_date); $i++) {
                    $chronology_date_part = $page_chronology_date[$i];
                    $page_chronology_date[$i] = is_numeric($chronology_date_part) ? (int) $chronology_date_part : 0;
                }
                needsList($page_chronology_date);
            }
            /** @param list<int|string> $x */
            function needsList(array $x): void {}
        