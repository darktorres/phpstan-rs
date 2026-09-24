<?php
            $start = new DateTime();
            $interval = new DateInterval('P1D');
            $end = new DateTime();
            new DatePeriod($start, $interval, $end);
        