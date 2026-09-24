<?php

trait Loggable
{
    protected string $logPrefix = "[log]";

    public function log(string $msg): string
    {
        return $this->logPrefix . " " . $msg;
    }
}

trait Timestamped
{
    public function stamp(): int
    {
        return 0;
    }
}

class Job
{
    use Loggable, Timestamped;

    public function run(): string
    {
        return $this->log("running") . (string) $this->stamp();
    }
}

$job = new Job();
echo $job->log("start");
echo $job->stamp();
