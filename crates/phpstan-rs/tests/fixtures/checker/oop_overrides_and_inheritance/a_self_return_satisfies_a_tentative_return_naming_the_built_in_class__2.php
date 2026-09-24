<?php
        class MyDateTime extends DateTime {
            public function modify(string $modifier): self { return $this; }
            public function setTimezone(DateTimeZone $timezone): self { return $this; }
        }
    