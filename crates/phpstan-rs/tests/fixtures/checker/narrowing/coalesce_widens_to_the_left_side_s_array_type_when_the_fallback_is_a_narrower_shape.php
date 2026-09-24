<?php
            class Row { public ?array $details; }
            function f(Row $row): void {
                $details = $row->details ?? [];
                echo $details['config_section'];
            }
        