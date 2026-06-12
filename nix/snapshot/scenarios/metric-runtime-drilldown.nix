{
  id = "metric-runtime-drilldown";
  title = "klocc GUI Metric Runtime Drilldown";
  label = "Metric, runtime filter, drilldown";
  description = "Switch to total reach, filter to runtime, then drill into xgcc's runtime dependency fanout.";

  screen = {
    width = 1920;
    height = 1080;
  };

  app = {
    width = 1760;
    height = 940;
  };

  artifacts = {
    perStepExtensions = ["png" "stats" "rects"];
    common = [
      "walkthrough.png"
      "walkthrough.stats"
      "klocc-gui.log"
      "environment.log"
    ];
  };

  steps = [
    {
      id = "total-reach";
      title = "Total Reach";
      label = "Metric total reach";
      description = "After cycling the metric selector from code LOC to total reach.";
      actions = [
        {
          op = "key";
          key = "m";
        }
        {
          op = "sleep";
          seconds = 1;
        }
      ];
    }
    {
      id = "runtime-filter";
      title = "Runtime Filter";
      label = "Filter runtime";
      description = "After cycling the filter selector from all entries to runtime entries.";
      actions = [
        {
          op = "key";
          key = "f";
        }
        {
          op = "sleep";
          seconds = 1;
        }
      ];
    }
    {
      id = "xgcc-runtime";
      title = "xgcc Runtime";
      label = "xgcc opened";
      description = "After clicking xgcc under the total reach runtime view.";
      actions = [
        {
          op = "click";
          point = [1586 489];
          button = "left";
        }
        {
          op = "sleep";
          seconds = 1;
        }
      ];
    }
  ];
}
